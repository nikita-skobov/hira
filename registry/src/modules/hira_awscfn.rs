#[hira::hira] use {};

#[allow(dead_code)]
const HIRA_MODULE_NAME: &'static str = "hira_awscfn";

pub const CFN_FILE: &'static str = "deploy.yml";

pub fn output_cfn_file(
    obj: &mut LibraryObj,
    parameter_names: &[String],
    cfn_resources: String,
) {
    let cfn_file = CFN_FILE;
    obj.append_to_file_unique(cfn_file, "# 0", "AWSTemplateFormatVersion: '2010-09-09'".into());
    obj.append_to_file_unique(cfn_file, "# 0", "Parameters:".into());
    obj.append_to_file_unique(cfn_file, "# 1", format!("    DefaultParam:\n        Type: String"));
    for param in parameter_names {
        obj.append_to_file(cfn_file, "# 1", format!("    {}:\n        Type: String", param));
    }
    obj.append_to_file_unique(cfn_file, "# 2", "Resources:".into());
    obj.append_to_file(cfn_file, "# 3", cfn_resources);
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

#[allow(dead_code)]
type ExportType = NotUsed;
pub struct NotUsed {}
pub fn wasm_entrypoint(_obj: &mut LibraryObj, _cb: fn(&mut NotUsed)) {}
