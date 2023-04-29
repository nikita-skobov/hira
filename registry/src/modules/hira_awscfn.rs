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

#[allow(dead_code)]
type ExportType = NotUsed;
pub struct NotUsed {}
pub fn wasm_entrypoint(_obj: &mut LibraryObj, _cb: fn(&mut NotUsed)) {}
