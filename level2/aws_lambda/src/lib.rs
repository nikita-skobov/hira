use aws_cfn::aws_cfn;
use hira_lib::level0::*;
use hira_lib::wasm_types::FunctionSignature;

#[hira::hira]
pub mod aws_lambda {
    use super::FunctionSignature;
    use super::aws_cfn;
    use super::L0CodeReader;
    use super::L0CodeWriter;
    use super::L0Core;
    use super::L0AppendFile;
    extern crate aws_regions;
    extern crate serde;

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[serde(default)]
    #[derive(Default)]
    pub struct FunctionUrlEvent {
        pub version: String,
        pub body: String,
        pub is_base64_encoded: bool,
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FunctionUrlResponse {
        pub status_code: u32,
        pub body: String,
        pub is_base64_encoded: bool,
        pub headers: std::collections::HashMap<String, String>,
    }

    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("CODE_READ", &["fn:lambda_main"]),
        ("CODE_WRITE", &["fn_module:service_func", "fn_global:main"]),
        ("FILES", &["deploy.sh"]),
    ];

    // #[derive(Default)]
    pub struct Input {
        /// logical name of the resource referenced in cloudformation.
        /// by default this is `Lambda{function_name}`.
        /// Must be alphanumeric, and up to 255 characters.
        pub resource_name: String,
        /// physical id of the lambda function. By default it is your function's name.
        /// Optionally set this to an empty string to get a randomly generated name.
        /// max 64 characters.
        pub function_name: String,

        /// the region this function will be deployed in. If specifying an s3 artifact bucket,
        /// the region of that bucket must match this region. Defaults to us-west-2
        pub region: String,

        /// memory to give your function (in MB). Defaults to 128.
        /// Valid values: 128 - 10240
        pub memory_size: u64,

        /// timeout of your function (in seconds). Defaults to 30.
        /// Valid values: 1 - 900
        pub timeout: u32,

        /// set to true if a function URL should
        /// be created for this lambda.
        pub use_event_function_url: bool,

        /// set to some if this lambda function should run on a schedule.
        /// first param is the value, and second param is the units.
        /// only valid units are `minutes`, `hours`, and `days`.
        /// the value must be positive and >= 1.
        /// For example set it to `Some((5, "minutes"))` to have your
        /// lambda be invoked every 5 minutes
        pub use_schedule: Option<(u32, String)>,
    }

    impl Default for Input {
        fn default() -> Self {
            Self {
                resource_name: "UNDEFINED_RESOURCE".into(),
                function_name: "UNDEFINED_FN_NAME".into(),
                region: "us-west-2".into(),
                memory_size: 128,
                timeout: 30,
                use_event_function_url: false,
                use_schedule: None,
            }
        }
    }

    impl Input {
        const RESOURCE_NAME_PREFIX: &'static str = "Lambda";
        const RESOURCE_NAME_PREFIX_LEN: usize = Self::RESOURCE_NAME_PREFIX.len();

        pub fn set_names(&mut self, users_mod_name: &str) {
            if self.function_name == "UNDEFINED_FN_NAME" {
                let mut func_name = users_mod_name.to_string();
                func_name = func_name.replace("_", "");
                // trim if longer than 64.
                let max_func_len = 64 - Self::RESOURCE_NAME_PREFIX_LEN;
                if func_name.len() > max_func_len {
                    func_name.truncate(max_func_len);
                }
                self.function_name = func_name;
            }
            if self.resource_name == "UNDEFINED_RESOURCE" {
                let resource_name = format!("{}{}", Self::RESOURCE_NAME_PREFIX, self.function_name);
                self.resource_name = resource_name;
            }
        }

        pub fn verify_and_output_cfn(&self) -> Result<(String, String, String), String> {
            match self.is_valid() {
                Some(err) => Err(err),
                None => Ok(self.output_cfn()),
            }
        }
        pub fn is_valid(&self) -> Option<String> {
            if let Some(err_msg) = aws_cfn::verify_resource_name(&self.resource_name) {
                return Some(err_msg);
            }
            if self.function_name.len() > 64 {
                return Some(format!("Invalid function name {:?}\nMust be at most 64 characters", self.function_name));
            }
            let region_err = aws_regions::verify_region(&self.region.as_str());
            if region_err.is_some() { return region_err }
            if self.memory_size < 128 || self.memory_size > 10240 {
                return Some(format!("Invalid memory size {:?}\nMust be between 128 and 10240", self.memory_size));
            }
            if self.timeout < 1 || self.timeout > 900 {
                return Some(format!("Invalid timeout {:?}\nMust be between 1 and 900", self.timeout));
            }
            if let Some((value, unit)) = &self.use_schedule {
                if unit != "minutes" && unit != "days" && unit != "hours" {
                    return Some(format!("Invalid unit {:?} for use_schedule config\nMust be between either `minutes`, `hours`, or `days`", unit));
                }
                if *value < 1 {
                    return Some(format!("Invalid value {:?} for use_schedule config. `value` must be >= 0", value));
                }
            }
            None
        }

        pub fn output_cfn(&self) -> (String, String, String) {
            let Self { resource_name, function_name, memory_size, timeout, use_event_function_url, use_schedule, .. } = self;
            let func_name = if function_name.is_empty() {
                "# FunctionName will be auto-generated".into()
            } else {
                format!("FunctionName: {function_name}")
            };

            let role_resource_name = format!("Role{resource_name}");


            let func_name_no_underscores = function_name.replace("_", "");
            let bucket_param = format!("ArtifactBucket{func_name_no_underscores}");
            let key_param = format!("ArtifactKey{func_name_no_underscores}");

            let mut x = format!(
    r#"    {resource_name}:
        Type: 'AWS::Lambda::Function'
        Properties:
            {func_name}
            Runtime: provided.al2
            Handler: index.handler
            Code:
                S3Bucket: !Ref {bucket_param}
                S3Key: !Ref {key_param}
            MemorySize: {memory_size}
            Timeout: {timeout}
            Architectures:
            - arm64
            Role: !GetAtt {role_resource_name}.Arn
    {role_resource_name}:
        Type: 'AWS::IAM::Role'
        Properties:
            AssumeRolePolicyDocument:
                Version: '2012-10-17'
                Statement:
                - Effect: Allow
                  Principal:
                      Service: lambda.amazonaws.com
                  Action:
                  - sts:AssumeRole
            ManagedPolicyArns:
            - 'arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole'
"#);
            if *use_event_function_url {
                x = format!(r#"{x}    LambdaTrigger{resource_name}:
        Type: AWS::Lambda::Url
        Properties:
            AuthType: NONE
            TargetFunctionArn: !GetAtt {resource_name}.Arn
    LambdaPermission{resource_name}:
        Type: AWS::Lambda::Permission
        Properties:
            Action: 'lambda:InvokeFunctionUrl'
            FunctionName: !GetAtt {resource_name}.Arn
            FunctionUrlAuthType: NONE
            Principal: '*'
"#);
            }
            if let Some((time, unit)) = use_schedule {
                x = format!(r#"{x}    Schedule{resource_name}:
        Type: AWS::Scheduler::Schedule
        Properties:
            FlexibleTimeWindow:
                Mode: 'OFF'
            ScheduleExpression: 'rate({time} {unit})'
            Target:
                Arn: !GetAtt {resource_name}.Arn
                RoleArn: !GetAtt ScheduleRole{resource_name}.Arn
    ScheduleRole{resource_name}:
        Type: AWS::IAM::Role
        Properties:
            AssumeRolePolicyDocument:
                Version: '2012-10-17'
                Statement:
                - Effect: Allow
                Principal:
                    Service: scheduler.amazonaws.com
                Action:
                - sts:AssumeRole
            Policies:
            - PolicyName: allow_schedule
            PolicyDocument:
                Version: '2012-10-17'
                Statement:
                - Sid: allowschedule
                    Effect: Allow
                    Action:
                    - lambda:InvokeFunction
                    Resource:
                    - !GetAtt {resource_name}.Arn
"#);
            }
            (x, bucket_param, key_param)
        }
    }

    fn validate_lambda_main_signature(sig: &FunctionSignature, use_event_func_url: &mut bool) -> Result<(String, String, String), String> {
        if sig.return_ty.is_empty() {
            return Err("Must provide a return type for your lambda_main function".into());
        }
        let return_ty = sig.return_ty.replace(" ", "");
        let (wrap_with_ok, b) = if return_ty.starts_with("Result<") {
            if !return_ty.ends_with("lambda_runtime::Error>") {
                return Err(format!("Your function uses an invalid result type '{return_ty}'. If returning a Result<> the error type must be 'lambda_runtime::Error'"));
            }
            (false, return_ty)
        } else {
            (true, format!("Result<{return_ty}, lambda_runtime::Error>"))
        };
    
        let input_param = if let Some(f) = sig.inputs.first() {
            f
        } else {
            return Err(format!("Expected 1 function parameter for fn lambda_main"));
        };
        if sig.inputs.len() != 1 {
            return Err(format!("Expected only 1 function parameter for fn lambda_main"));
        }
        let input_param_type = &input_param.ty;
        if input_param_type == "aws_lambda :: FunctionUrlEvent" {
            *use_event_func_url = true;
        }
        let mut return_statement = if sig.is_async {
            format!("lambda_main(x).await")
        } else {
            format!("lambda_main(x)")
        };
        if wrap_with_ok {
            return_statement = format!("Ok({return_statement})");
        }
        Ok((return_statement, b, input_param_type.to_string()))
    }

    pub fn config(
        lambda_input: &mut Input,
        cfninput: &mut aws_cfn::Input,
        l0core: &mut L0Core,
        l0code: &mut L0CodeReader,
        l0write: &mut L0CodeWriter,
        l0append: &mut L0AppendFile,
    ) {
        let user_mod_name = l0core.users_module_name();
        lambda_input.set_names(&user_mod_name);
        let lambda_main_signature = match l0code.get_fn("lambda_main") {
            Some(sig) => sig,
            None => {
                l0core.compiler_error("Failed to find lambda_main function");
                return;
            }
        };

        let (return_statement, return_ty, input_param_type) = match validate_lambda_main_signature(lambda_main_signature, &mut lambda_input.use_event_function_url) {
            Err(e) => {
                l0core.compiler_error(&e);
                return;
            }
            Ok(p) => p,
        };
        let (cfn_resources, bucket_param, key_param) = match lambda_input.verify_and_output_cfn() {
            Ok(out) => out,
            Err(e) => {
                l0core.compiler_error(&e);
                return;
            }
        };
        l0write.write_internal_fn(
            format!("pub async fn service_func(event: lambda_runtime::LambdaEvent<{}>) -> {}", input_param_type, return_ty),
            format!("let (x, _context) = event.into_parts(); {}", return_statement)
        );
        l0write.write_global_fn(
            format!("#[cfg({user_mod_name})]\n#[tokio::main]\nasync fn main() -> Result<(), lambda_runtime::Error>"),
            format!("let func = lambda_runtime::service_fn({user_mod_name}::service_func);\nlambda_runtime::run(func).await?;\nOk(())")
        );

        let region = &lambda_input.region;
        let region_underscores = region.replace("-", "_");
        let target_dir = format!("target_{user_mod_name}");
        let crate_name = l0core.crate_name();
        let random_name_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then randomid=($(echo $(md5sum ../* 2>&1) | md5sum)); artifactbucketname_{region_underscores}=\"hiraartifacts-$randomid\"; fi");
        let create_deploy_bucket_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then aws s3api create-bucket --bucket \"$artifactbucketname_{region_underscores}\" --create-bucket-configuration LocationConstraint={region}; fi");
        let save_bucket_name_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then echo \"$artifactbucketname_{region_underscores}\" > ./s3artifactbucket_{region_underscores}.txt; fi");
        let get_artifact_bucket_name = format!("artifactbucketname{user_mod_name}=$(< ./s3artifactbucket_{region_underscores}.txt)");
        let target = "aarch64-unknown-linux-musl"; // TODO: allow user customizing this
        let compilecmd = format!("CARGO_WASMTYPEGEN_FILEOPS=\"0\" RUSTFLAGS=\"--cfg {user_mod_name}\" cross rustc --crate-type=bin --release --target {target} --target-dir {target_dir}");
        let copycmd = format!("cp ./{target_dir}/{target}/release/{crate_name} ./bootstrap");
        let md5cmd = format!("md5{user_mod_name}=($(md5sum ./bootstrap))");
        let zipcmd = format!("zip -r {user_mod_name}_$md5{user_mod_name}.zip bootstrap");
        let deployartifactcmd = format!("aws s3 cp {user_mod_name}_$md5{user_mod_name}.zip \"s3://$artifactbucketname{user_mod_name}/\"");

        let deploy_file = "deploy.sh";
        let pre_build = "# 0. pre-build:";
        let build = "# 1. build:";
        let package = "# 2. package:";
    
        l0append.append_to_file_unique(deploy_file, pre_build, random_name_cmd);
        l0append.append_to_file_unique(deploy_file, pre_build, create_deploy_bucket_cmd);
        l0append.append_to_file_unique(deploy_file, pre_build, save_bucket_name_cmd);
        l0append.append_to_file(deploy_file, build, compilecmd);
        l0append.append_to_file(deploy_file, build, copycmd);
        l0append.append_to_file(deploy_file, build, md5cmd);
        l0append.append_to_file(deploy_file, build, zipcmd);
        l0append.append_to_file(deploy_file, package, get_artifact_bucket_name.into());
        l0append.append_to_file(deploy_file, package, deployartifactcmd);

        let params = [
            (bucket_param, format!("$artifactbucketname{user_mod_name}")),
            (key_param, format!("{user_mod_name}_$md5{user_mod_name}.zip")),
        ];

        cfninput.region = region.to_string();
        cfninput.parameters = params.to_vec();
        cfninput.cfn_resources = cfn_resources;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

}
