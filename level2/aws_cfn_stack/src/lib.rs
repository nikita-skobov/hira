use std::collections::HashMap;

use hira_lib::level0::*;
use aws_config;
use aws_sdk_cloudformation::{self, types::{Stack, Capability, OnFailure, StackResourceSummary}};

use crate::aws_cfn_stack::SavedTemplate;

pub async fn runtime_main(data: &Vec<String>) {
    // // TODO: allow user to customize region.
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_cloudformation::Client::new(&shared_config);
    let mut stack_map: HashMap<String, Vec<(String, aws_cfn_stack::SavedTemplate)>> = HashMap::new();
    let mut num_resources = 0;
    for stack_str in data {
        let stack: aws_cfn_stack::SavedStack = cfn_resources::serde_json::from_str(&stack_str).expect("Failed to deserialize generated json file");
        for (stack_name, (mod_name, template)) in stack.template {
            num_resources += template.resources.len();
            if let Some(existing) = stack_map.get_mut(&stack_name) {
                existing.push((mod_name, template));
            } else {
                stack_map.insert(stack_name, vec![(mod_name, template)]);
            }
            // stack.template is guaranteed to only have 1 template, we can break here
            break;
        }
    }
    println!("\nDeploying {} resource(s)", num_resources);
    println!("Across {} stack(s)", stack_map.len());

    for (stack_name, templates) in stack_map {
        println!("\nAbout to deploy stack: {stack_name}");
        let mut final_template = SavedTemplate::default();
        let mut module_resources: HashMap<String, (ModResourceCounts, Vec<(bool, String)>)> = HashMap::new();
        for (mod_name, template) in templates {
            for (resource_name, _) in template.resources.iter() {
                if let Some((_, existing)) = module_resources.get_mut(&mod_name) {
                    existing.push((false, resource_name.to_string()));
                } else {
                    let mod_resource_counts = ModResourceCounts {
                        complete_count: 0,
                        has_changes: true,
                    };
                    module_resources.insert(mod_name.to_string(), (mod_resource_counts, vec![(false, resource_name.to_string())]));
                }
            }
            final_template.resources.extend(template.resources);
            final_template.outputs.extend(template.outputs);
        }
        // we make it pretty so if a user needs to look at the stack in Cfn console, it looks nice
        let template_body = cfn_resources::serde_json::to_string_pretty(&final_template).expect("Failed to serialize template");
        if let Err(e) = create_or_update_stack(&client, &stack_name, &template_body).await {
            panic!("Failed to create stack {stack_name}\n{e}");
        }
        let mut outputs = match wait_for_output(&client, &stack_name, Some(&mut module_resources)).await {
            Err(e) => panic!("Failed to create stack {stack_name}\n{e}"),
            Ok(o) => o,
        };
        let mut outputs: Vec<(String, String)> = outputs.drain().map(|(k, v)| {
            (k, v)
        }).collect();
        outputs.sort_by(|a, b| a.0.cmp(&b.0));
        if !outputs.is_empty() {
            println!("\nOutputs:");
            for (key, val) in outputs {
                println!("- {}:\n  {}", key, val);
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
            let dne_error = format!("Stack with id {name} does not exist");
            let e_str = format!("{:#?}", e);
            // we consider this still in progress, since we're waiting for it to show up in the API.
            if e_str.contains(&dne_error) {
                return Ok(None);
            }
            return Err(e_str);
        }
    }
}

pub async fn get_all_stack_resources(
    client: &aws_sdk_cloudformation::Client, name: &str,
    mut next_token: Option<String>,
) -> Result<Vec<StackResourceSummary>, String> {
    let mut append = vec![];
    loop {
        let mut builder = client
            .list_stack_resources().stack_name(name);
        if let Some(s) = next_token {
            builder = builder.next_token(s);
        }
        let resp = builder.send().await.map_err(|e| e.to_string())?;
        
        let list = resp.stack_resource_summaries().unwrap_or_default();
        for item in list {
            append.push(item.clone());
        }
        if let Some(nt) = resp.next_token() {
            next_token = Some(nt.to_string());
        } else {
            break;
        }
    }
    Ok(append)
}

pub struct ModResourceCounts {
    pub complete_count: u32,
    pub has_changes: bool,
}

impl ModResourceCounts {
    pub fn set_complete_count(&mut self, num_complete: u32) {
        if self.complete_count != num_complete {
            self.has_changes = true;
        }
        self.complete_count = num_complete;
    }

    pub fn print(&mut self, mod_name: &str, total_resources: usize) {
        if self.has_changes {
            println!("{mod_name}\t\t{}/{}", self.complete_count, total_resources);
        }
        self.has_changes = false;
    }
}

pub async fn wait_for_output(
    client: &aws_sdk_cloudformation::Client, name: &str,
    mut module_resources: Option<&mut HashMap<String, (ModResourceCounts, Vec<(bool, String)>)>>,
) -> Result<HashMap<String, String>, String> {
    loop {
        let dur = tokio::time::Duration::from_millis(700);
        tokio::time::sleep(dur).await;
        let stack_resp = describe_stack(client, name).await?;
        match stack_resp {
            Some(stack) => {
                let mut out = HashMap::new();
                for output in stack.outputs().unwrap_or_default() {
                    match (output.output_key(), output.output_value()) {
                        (Some(key), Some(val)) => {
                            out.insert(key.to_string(), val.to_string());
                        }
                        _ => {}
                    }
                }
                if let Some(mod_data) = &mut module_resources {
                    for (mod_name, (counts, resources)) in mod_data.iter_mut() {
                        let num_resources = resources.len();
                        counts.set_complete_count(num_resources as _);
                        counts.print(mod_name, num_resources);
                    }
                }
                return Ok(out);
            }
            None => {
                // only print if module resources provided:
                let mod_resources = if let Some(r) = &mut module_resources {
                    r
                } else {
                    continue;
                };
                // should print all 0s first before we get the data
                for (mod_name, (counts, resources)) in mod_resources.iter_mut() {
                    counts.print(&mod_name, resources.len());
                }

                // stack not ready yet.
                // print output of all the modules and how many
                // of their resources have been created
                // this is best effort, so if we fail, we just ignore
                let all_resources = if let Ok(o) = get_all_stack_resources(client, name, None).await {
                    o
                } else {
                    continue;
                };
                // build a map of logical resource names
                // to their status
                let mut map = HashMap::new();
                for resource in all_resources.iter() {
                    if let Some(id) = resource.logical_resource_id() {
                        if let Some(status) = resource.resource_status() {
                            map.insert(id, status);
                        }
                    }
                }
                // finally, iterate and print
                for (mod_name, (counts, resource_ids)) in mod_resources.iter_mut() {
                    let num_resources = resource_ids.len();
                    // count how many of the resource ids are complete:
                    let mut complete_count = 0;
                    for (_, id) in resource_ids {
                        if let Some(status) = map.get(id.as_str()) {
                            match status {
                                // TODO: should allow other statuses to count as complete?
                                // update complete? etc.
                                aws_sdk_cloudformation::types::ResourceStatus::CreateComplete => {
                                    complete_count += 1;
                                }
                                _ => {}
                            }
                        }
                    }
                    counts.set_complete_count(complete_count);
                    counts.print(&mod_name, num_resources);
                }
            }
        }
    }
}

pub async fn create_or_update_stack(client: &aws_sdk_cloudformation::Client, name: &str, body: &str) -> Result<(), String> {
    let exists = does_stack_exist(client, name).await?;
    if exists {
        println!("Updating {name} ...");
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
                let e_str = format!("Failed to update:\n{:#?}", e);
                if e_str.contains("No updates are to be performed") {
                    return Ok(())
                }
                return Err(e_str)
            }
        }
    } else {
        println!("Creating {name} ...");
        // create
        client
            .create_stack()
            .on_failure(OnFailure::Delete)
            .capabilities(Capability::CapabilityNamedIam)
            .capabilities(Capability::CapabilityIam)
            .stack_name(name)
            .template_body(body)
            .send()
            .await.map_err(|e| format!("Failed to create:\n{:#?}", e))?;
    }
    Ok(())
}

/// This module is a low level module built to enable easily creating other modules on top of it.
/// To use this module you provide a list of Resources, where each Resource contains one or more
/// cloudformation resource definitions. This module then saves all of the inputs across
/// all invocations, and creates a runtime that will deploy one or many cloudformation stacks
/// using the resources you defined.
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

    #[derive(Debug, Clone, cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
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
        pub template: std::collections::HashMap<String, (String, SavedTemplate)>,
    }

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct Input {
        /// if left empty (default), we set the stack name to `hira-gen-default-stack`
        /// Optionally, provide a stack name of your own. You can group resources into 1 stack by ensuring
        /// all of the stack names are the same.
        pub stack_name: String,
        #[cfg_attr(feature = "web", serde(skip))]
        pub resources: Vec<Resource>,
        /// a list of function invocations that should be ran
        /// prior to deploying the stack.
        pub run_before: Vec<String>,
        pub outputs: std::collections::HashMap<String, ResourceOutput>,
    }

    fn validate_resources_to_template(resources: &Vec<Resource>, outputs: &std::collections::HashMap<String, ResourceOutput>) -> Result<SavedTemplate, String> {
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
        out_template.outputs = outputs.clone();
        Ok(out_template)
    }

    fn get_serialized_stack_json(user_mod_name: String, stack_name: &String, template: SavedTemplate) -> Result<String, String> {
        let mut stack = SavedStack::default();
        stack.template.insert(stack_name.clone(), (user_mod_name, template));
        match cfn_resources::serde_json::to_string(&stack) {
            Err(e) => {
                Err(format!("Failed to serialize template\n{:#?}", e))
            }
            Ok(o) => Ok(o)
        }
    }

    fn validate_stack_name(_user_mod_name: &str, current_stack_name: &str) -> Result<String, String> {
        let stack_name = if current_stack_name.is_empty() {
            "hira-gen-default-stack".to_string()
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
        let out_template = match validate_resources_to_template(&input.resources, &input.outputs) {
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
        let output = match get_serialized_stack_json(user_mod_name, &stack_name, out_template) {
            Ok(s) => s,
            Err(e) => {
                return core.compiler_error(&e);
            }
        };

        for code in input.run_before.iter() {
            runtimer.add_to_runtime_unique_beginning("deploy", code.to_string());
        }
        runtimer.add_to_runtime_unique_end("deploy", "::aws_cfn_stack::runtime_main(&runtime_data).await".to_string());
        runtimer.add_data_to_runtime("deploy", output);
    }
}
