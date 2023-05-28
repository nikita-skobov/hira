use std::{io::Write, collections::HashMap};

use hira_lib::level0::*;
use aws_config;
use aws_sdk_cloudformation::{self, types::{Stack, Capability, OnFailure}};

pub async fn runtime_main(data: &Vec<String>) {
    // // TODO: allow user to customize region.
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_cloudformation::Client::new(&shared_config);

    for stack_str in data {
        let stack: aws_cfn_stack::SavedStack = cfn_resources::serde_json::from_str(&stack_str).expect("Failed to deserialize generated json file");
        // TODO: merge resources based on stack names
        for (stack_name, template) in stack.template.iter() {
            println!("About to deploy stack: {stack_name}");
            for (_, resource) in template.resources.iter() {
                println!("{:#?}", resource.properties);
            }
            // we make it pretty so if a user needs to look at the stack in Cfn console, it looks nice
            let template_body = cfn_resources::serde_json::to_string_pretty(template).expect("Failed to serialize template");
            if let Err(e) = create_or_update_stack(&client, stack_name, &template_body).await {
                panic!("Failed to create stack {stack_name}\n{e}");
            }
            if let Err(e) = wait_for_output(&client, &stack_name).await {
                panic!("Failed to create stack {stack_name}\n{e}");
            }
        }
    }
}

pub async fn does_stack_exist(client: &aws_sdk_cloudformation::Client, name: &str) -> Result<bool, String> {
    // does not exist
    match client.describe_stacks().stack_name(name).send().await {
        Ok(_) => return Ok(true),
        Err(e) => {
            let e_str = format!("{:#?}", e);
            if e_str.contains("does not exist") {
                return Ok(false);
            }
            return Err(e_str);
        }
    }
}


pub async fn describe_stack(client: &aws_sdk_cloudformation::Client, name: &str) -> Result<Option<Stack>, String> {
    match client.describe_stacks().stack_name(name).send().await {
        Ok(d) => {
            if let Some(stacks) = d.stacks() {
                if let Some(first) = stacks.first() {
                    match first.stack_status() {
                        Some(status) => match status {
                            // done and return success:
                            aws_sdk_cloudformation::types::StackStatus::DeleteComplete |
                            aws_sdk_cloudformation::types::StackStatus::CreateComplete |
                            aws_sdk_cloudformation::types::StackStatus::UpdateComplete |
                            aws_sdk_cloudformation::types::StackStatus::UpdateRollbackComplete |
                            aws_sdk_cloudformation::types::StackStatus::ImportComplete |
                            aws_sdk_cloudformation::types::StackStatus::ImportRollbackComplete => {
                                return Ok(Some(first.clone()));
                            }

                            // keep trying
                            aws_sdk_cloudformation::types::StackStatus::CreateInProgress |
                            aws_sdk_cloudformation::types::StackStatus::DeleteInProgress |
                            aws_sdk_cloudformation::types::StackStatus::ImportInProgress |
                            aws_sdk_cloudformation::types::StackStatus::ImportRollbackInProgress |
                            aws_sdk_cloudformation::types::StackStatus::ReviewInProgress |
                            aws_sdk_cloudformation::types::StackStatus::RollbackInProgress |
                            aws_sdk_cloudformation::types::StackStatus::UpdateCompleteCleanupInProgress |
                            aws_sdk_cloudformation::types::StackStatus::UpdateInProgress |
                            aws_sdk_cloudformation::types::StackStatus::UpdateRollbackCompleteCleanupInProgress |
                            aws_sdk_cloudformation::types::StackStatus::UpdateRollbackInProgress => {
                                return Ok(None)
                            }

                            aws_sdk_cloudformation::types::StackStatus::RollbackComplete |
                            aws_sdk_cloudformation::types::StackStatus::CreateFailed |
                            aws_sdk_cloudformation::types::StackStatus::DeleteFailed |
                            aws_sdk_cloudformation::types::StackStatus::ImportRollbackFailed |
                            aws_sdk_cloudformation::types::StackStatus::RollbackFailed |
                            aws_sdk_cloudformation::types::StackStatus::UpdateFailed |
                            aws_sdk_cloudformation::types::StackStatus::UpdateRollbackFailed |
                            _ => {
                                return Err(first.stack_status_reason().unwrap_or("Failed to get stack failure reason").to_string())
                            }
                        }
                        None => {
                            return Err(format!("Stack {name} not found"))
                        }
                    }
                } else {
                    return Err(format!("Stack {name} not found"))
                }
            } else {
                return Err(format!("Stack {name} not found"))
            }
        }
        Err(e) => {
            let e_str = format!("{:#?}", e);
            return Err(e_str);
        }
    }
}


pub async fn wait_for_output(client: &aws_sdk_cloudformation::Client, name: &str) -> Result<HashMap<String, String>, String> {
    loop {
        let dur = tokio::time::Duration::from_millis(700);
        tokio::time::sleep(dur).await;
        let stack_resp = describe_stack(client, name).await?;
        match stack_resp {
            Some(stack) => {
                println!("");
                let mut out = HashMap::new();
                for output in stack.outputs().unwrap_or_default() {
                    match (output.output_key(), output.output_value()) {
                        (Some(key), Some(val)) => {
                            out.insert(key.to_string(), val.to_string());
                        }
                        _ => {}
                    }
                }
                return Ok(out);
            }
            None => {
                // still waiting
                print!(".");
                let _ = std::io::stdout().flush();
            }
        }
    }
}

pub async fn create_or_update_stack(client: &aws_sdk_cloudformation::Client, name: &str, body: &str) -> Result<(), String> {
    let exists = does_stack_exist(client, name).await?;
    if exists {
        print!("Updating {name} ...");
        let _ = std::io::stdout().flush();
        // update
        match client
            .update_stack()
            .capabilities(Capability::CapabilityNamedIam)
            .capabilities(Capability::CapabilityIam)
            .stack_name(name)
            .template_body(body)
            .send()
            .await
        {
            Ok(_) => {},
            Err(e) => {
                let e_str = format!("{:#?}", e);
                if e_str.contains("No updates are to be performed") {
                    return Ok(())
                }
                return Err(e_str)
            }
        }
    } else {
        print!("Creating {name} ...");
        let _ = std::io::stdout().flush();
        // create
        client
            .create_stack()
            .on_failure(OnFailure::Delete)
            .capabilities(Capability::CapabilityNamedIam)
            .capabilities(Capability::CapabilityIam)
            .stack_name(name)
            .template_body(body)
            .send()
            .await.map_err(|e| format!("{:#?}", e))?;
    }
    Ok(())
}

#[hira::hira]
pub mod aws_cfn_stack {
    extern crate cfn_resources;
    use super::L0Core;
    use super::L0RuntimeCreator;

    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("RUNTIME", &[""]),
    ];

    pub struct Resource {
        pub name: String,
        pub properties: Box<dyn cfn_resources::CfnResource>,
    }

    #[derive(Debug, Default, cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    pub struct SavedResource {
        #[serde(rename = "Type")]
        pub ty: String,
        #[serde(rename = "Properties")]
        pub properties: cfn_resources::serde_json::Value,
    }

    #[derive(Debug, cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    pub struct SavedTemplate {
        #[serde(rename = "AWSTemplateFormatVersion")]
        pub version: String,
        #[serde(rename = "Resources")]
        pub resources: std::collections::HashMap<String, SavedResource>,
        #[serde(rename = "Outputs")]
        pub outputs: std::collections::HashMap<String, ResourceOutput>,
    }

    #[derive(Debug, cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    pub struct ResourceOutput {
        #[serde(rename = "Description")]
        pub description: String,
        #[serde(rename = "Value")]
        pub value: cfn_resources::serde_json::Value,
        // TODO: add condition/export?
    }

    impl Default for SavedTemplate {
        fn default() -> Self {
            Self {
                version: "2010-09-09".to_string(),
                resources: Default::default(),
                outputs: Default::default()
            }
        }
    }

    #[derive(Default, cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    pub struct SavedStack {
        /// this is expected to only have 1 item.
        /// we structure it this way so that we can separate the stack name
        /// from the template
        pub template: std::collections::HashMap<String, SavedTemplate>,
    }

    #[derive(Default)]
    pub struct Input {
        /// if left empty (default), we will use the name of your module
        /// as the stack name.
        pub stack_name: String,
        pub resources: Vec<Resource>,
        /// a list of function invocations that should be ran
        /// prior to deploying the stack.
        pub run_before: Vec<String>,
    }

    fn validate_resources_to_template(resources: &Vec<Resource>) -> Result<SavedTemplate, String> {
        let mut out_template = SavedTemplate::default();
        for resource in resources.iter() {
            if let Err(e) = resource.properties.validate() {
                return Err(format!("Validation failed on resource '{}'\n{e}", resource.name));
            }
            let saved_resource = SavedResource {
                ty: resource.properties.type_string().to_string(),
                properties: resource.properties.properties(),
            };
            out_template.resources.insert(resource.name.clone(), saved_resource);
        }
        Ok(out_template)
    }

    fn get_serialized_stack_json(stack_name: &String, template: SavedTemplate) -> Result<String, String> {
        let mut stack = SavedStack::default();
        stack.template.insert(stack_name.clone(), template);
        match cfn_resources::serde_json::to_string(&stack) {
            Err(e) => {
                Err(format!("Failed to serialize template\n{:#?}", e))
            }
            Ok(o) => Ok(o)
        }
    }

    fn validate_stack_name(user_mod_name: &str, current_stack_name: &str) -> Result<String, String> {
        let stack_name = if current_stack_name.is_empty() {
            let mut stack_name = user_mod_name.to_string();
            stack_name = stack_name.replace("_", "-");
            stack_name.truncate(128);
            stack_name
        } else {
            current_stack_name.to_string()
        };
        // A stack name can contain only alphanumeric characters (case sensitive) and hyphens.
        // It must start with an alphabetical character and can't be longer than 128 characters.
        let restricion = "Must only consist of alphanumeric characters and hyphens, Must start with an alphabetical character, and cannot be longer than 128 characters.";
        for (i, c) in stack_name.chars().enumerate() {
            if i == 0 {
                if !c.is_ascii_alphabetic() {
                    return Err(format!("Invalid stack name {}\n{}", stack_name, restricion));
                }
            }
            if !c.is_ascii_alphanumeric() && c != '-' {
                return Err(format!("Invalid stack name {}\n{}", stack_name, restricion));
            }
        }
        if stack_name.len() > 128 {
            return Err(format!("Invalid stack name {}\n{}", stack_name, restricion));
        }
        Ok(stack_name)
    }

    pub fn config(input: &mut Input, core: &mut L0Core, runtimer: &mut L0RuntimeCreator) {
        let out_template = match validate_resources_to_template(&input.resources) {
            Ok(t) => t,
            Err(e) => {
                return core.compiler_error(&e);
            }
        };
        let user_mod_name = core.users_module_name();
        let stack_name = match validate_stack_name(&user_mod_name, &input.stack_name) {
            Ok(s) => s,
            Err(e) => {
                return core.compiler_error(&e);
            }
        };
        let output = match get_serialized_stack_json(&stack_name, out_template) {
            Ok(s) => s,
            Err(e) => {
                return core.compiler_error(&e);
            }
        };

        for code in input.run_before.iter() {
            runtimer.add_to_runtime_unique_beginning("deployer", code.to_string());
        }
        runtimer.add_to_runtime_unique_end("deployer", "aws_cfn_stack::runtime_main(&runtime_data).await".to_string());
        runtimer.add_data_to_runtime("deployer", output);
    }
}
