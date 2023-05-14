use hira_lib::level0::L0AppendFile;


#[hira::hira]
pub mod aws_cfn {
    use super::L0AppendFile;

    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("FILES", &["deploy.yml", "deploy.sh"])
    ];

    pub const CFN_FILE: &'static str = "deploy.yml";
    pub const DEPLOY_FILE: &'static str = "deploy.sh";
    pub const STEP_DEPLOY: &'static str = "# 3. deploy:";


    #[derive(Default)]
    pub struct Input {
        pub region: String,
        pub parameters: Vec<(String, String)>,
        pub cfn_resources: String,
    }

    pub fn verify_resource_name(resource_name: &str) -> Option<String> {
        if resource_name.len() > 255 {
            return Some(format!("Invalid resource name {:?}\nmust be less than 255 characters", resource_name));
        }
        if resource_name.len() < 1 {
            return Some(format!("Invalid resource name {:?}\nMust contain at least 1 character", resource_name));
        }
        if !resource_name.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(format!("Invalid resource name {:?}\nMust contain only alphanumeric characters [A-Za-z0-9]", resource_name));
        }
        None
    }

    pub fn config(input: &mut Input, obj: &mut L0AppendFile) {
        let Input { region, parameters, cfn_resources } = input;
        let cfn_resources = std::mem::take(cfn_resources);

        let cfn_file = CFN_FILE;
        let deploy_file = DEPLOY_FILE;

        let deploycfncmd = format!("AWS_REGION=\"{}\" aws --region {} cloudformation deploy --stack-name hira-gen-stack --template-file deploy.yml --capabilities CAPABILITY_NAMED_IAM --parameter-overrides DefaultParam=hira ", region, region);

        let mut out_param_str = "".to_string();
        for (param_name, param_value) in parameters.iter() {
            out_param_str.push_str(&format!("{}={} ", param_name, param_value));
        }
        obj.append_to_line(deploy_file, STEP_DEPLOY, deploycfncmd, out_param_str);
        
        obj.append_to_file_unique(cfn_file, "# 0", "AWSTemplateFormatVersion: '2010-09-09'".into());
        obj.append_to_file_unique(cfn_file, "# 0", "Parameters:".into());
        obj.append_to_file_unique(cfn_file, "# 1", format!("    DefaultParam:\n        Type: String"));
        for (param, _) in parameters {
            obj.append_to_file(cfn_file, "# 1", format!("    {}:\n        Type: String", param));
        }
        obj.append_to_file_unique(cfn_file, "# 2", "Resources:".into());
        obj.append_to_file(cfn_file, "# 3", cfn_resources);
    }
}
