#[hira::hira] mod _typehints {}


#[derive(Default)]
pub struct LambdaInput {
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
}

pub const REQUIRED_CRATES: &[&'static str] = &["tokio", "lambda_runtime"];

pub type ExportType = LambdaInput;

const VALID_AWS_REGIONS: &[&'static str] = &[
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "ca-central-1",
    "eu-north-1",
    "eu-west-3",
    "eu-west-2",
    "eu-west-1",
    "eu-central-1",
    "eu-south-1",
    "ap-south-1",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-3",
    "ap-east-1",
    "sa-east-1",
    "cn-north-1",
    "cn-northwest-1",
    "us-gov-east-1",
    "us-gov-west-1",
    "us-gov-secret-1",
    "us-gov-topsecret-1",
    "us-gov-topsecret-2",
    "me-south-1",
    "af-south-1",
];

impl LambdaInput {
    const RESOURCE_NAME_PREFIX: &'static str = "Lambda";
    const RESOURCE_NAME_PREFIX_LEN: usize = Self::RESOURCE_NAME_PREFIX.len();

    pub fn new(obj: &mut LibraryObj) -> Self {
        let mut out = Self::default();
        let mut func_name = obj.user_data.get_name().clone();
        // trim if longer than 64.
        let max_func_len = 64 - Self::RESOURCE_NAME_PREFIX_LEN;
        if func_name.len() > max_func_len {
            func_name.truncate(max_func_len);
        }
        let resource_name = format!("{}{}", Self::RESOURCE_NAME_PREFIX, func_name);
        out.resource_name = resource_name;
        out.function_name = func_name;
        out.region = "us-west-2".into();
        out.memory_size = 128;
        out.timeout = 30;
        out
    }
    pub fn verify_and_output_cfn(&self, obj: &mut LibraryObj) -> Option<(String, String, String)> {
        if !self.is_valid(obj) {
            return None;
        }
        Some(self.output_cfn())
    }
    pub fn is_valid(&self, obj: &mut LibraryObj) -> bool {
        if self.resource_name.len() > 255 {
            obj.compile_error(&format!("Invalid resource name {:?}\nmust be less than 255 characters", self.resource_name));
            return false;
        }
        if self.resource_name.len() < 1 {
            obj.compile_error(&format!("Invalid resource name {:?}\nMust contain at least 1 character", self.resource_name));
            return false;
        }
        if !self.resource_name.chars().all(|c| c.is_ascii_alphanumeric()) {
            obj.compile_error(&format!("Invalid resource name {:?}\nMust contain only alphanumeric characters [A-Za-z0-9]", self.resource_name));
            return false;
        }
        if self.function_name.len() > 64 {
            obj.compile_error(&format!("Invalid function name {:?}\nMust be at most 64 characters", self.function_name));
            return false;
        }
        if !VALID_AWS_REGIONS.contains(&self.region.as_str()) {
            obj.compile_error(&format!("Invalid region code {:?}\nMust be one of {:?}", self.region, VALID_AWS_REGIONS));
            return false;
        }
        if self.memory_size < 128 || self.memory_size > 10240 {
            obj.compile_error(&format!("Invalid memory size {:?}\nMust be between 128 and 10240", self.memory_size));
            return false;
        }
        if self.timeout < 1 || self.timeout > 900 {
            obj.compile_error(&format!("Invalid timeout {:?}\nMust be between 1 and 900", self.timeout));
            return false;
        }

        true
    }

    pub fn output_cfn(&self) -> (String, String, String) {
        let Self { resource_name, function_name, memory_size, timeout, .. } = self;
        let func_name = if function_name.is_empty() {
            "# FunctionName will be auto-generated".into()
        } else {
            format!("FunctionName: {function_name}")
        };

        let role_resource_name = format!("Role{resource_name}");


        let func_name_no_underscores = function_name.replace("_", "");
        let bucket_param = format!("ArtifactBucket{func_name_no_underscores}");
        let key_param = format!("ArtifactKey{func_name_no_underscores}");

        let x = format!(
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
        (x, bucket_param, key_param)
    }
}

pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut LambdaInput)) {
    let mut lambda_input = LambdaInput::new(obj);
    // call the user's callback to let them modify the default lambda config
    cb(&mut lambda_input);
    let (cfn_resources, bucket_param, key_param) = if let Some(x) = lambda_input.verify_and_output_cfn(obj) {
        x
    } else {
        return;
    };

    let (name, is_async, inputs, return_ty) = match &obj.user_data {
        UserData::Function { name, is_async, inputs, return_ty, .. } => {
            (name, is_async, inputs, return_ty)
        }
        _ => {
            obj.compile_error("This module can only be applied to a function");
            return;
        }
    };
    if return_ty.is_empty() {
        obj.compile_error("Must provide a return type on your function");
        return;
    }
    let return_ty = return_ty.replace(" ", "");
    let (wrap_with_ok, return_ty) = if return_ty.starts_with("Result<") {
        if !return_ty.ends_with("lambda_runtime::Error>") {
            obj.compile_error(&format!("Your function uses an invalid result type '{return_ty}'. If returning a Result<> the error type must be 'lambda_runtime::Error'"));
            return;
        }
        (false, return_ty)
    } else {
        (true, format!("Result<{return_ty}, lambda_runtime::Error>"))
    };

    let service_func_name = format!("service_func_{name}");
    let users_func_name = name;
    let region = &lambda_input.region;
    let region_underscores = region.replace("-", "_");

    let input_param = if let Some(f) = inputs.first() {
        f
    } else {
        obj.compile_error(&format!("Expected 1 function parameter for fn {name}"));
        return;
    };
    if inputs.len() != 1 {
        obj.compile_error(&format!("Expected only 1 function parameter for fn {name}"));
        return;
    }
    let input_param_type = &input_param.ty;
    let mut return_statement = if *is_async {
        format!("{users_func_name}(x).await")
    } else {
        format!("{users_func_name}(x)")
    };
    if wrap_with_ok {
        return_statement = format!("Ok({return_statement})");
    }

    let service_func_def = stringify!(
        #[allow(dead_code)]
        async fn service_func_name(event: lambda_runtime::LambdaEvent<input_param_type>) -> return_ty {
            let (x, _context) = event.into_parts();
            return_statement
        }
    );
    let service_func_def = service_func_def.replace("input_param_type", input_param_type);
    let service_func_def = service_func_def.replace("return_ty", &return_ty);
    let service_func_def = service_func_def.replace("service_func_name", &service_func_name);
    let service_func_def = service_func_def.replace("return_statement", &return_statement);

    let main_func_def = stringify!(
        #[cfg(CFG_NAME)]
        #[tokio::main]
        async fn main() -> Result<(), lambda_runtime::Error> {
            let func = lambda_runtime::service_fn(service_func_name);
            lambda_runtime::run(func).await?;
            Ok(())
        }
    );
    let main_func_def = main_func_def.replace("CFG_NAME", users_func_name);
    let main_func_def = main_func_def.replace("service_func_name", &service_func_name);
    obj.add_code_after.push(main_func_def);
    obj.add_code_after.push(service_func_def);

    let target_dir = format!("target_{users_func_name}");
    let crate_name = obj.crate_name.clone();
    let random_name_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then randomid=($(echo $(md5sum ../* 2>&1) | md5sum)); artifactbucketname_{region_underscores}=\"hiraartifacts-$randomid\"; fi");
    let create_deploy_bucket_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then aws s3api create-bucket --bucket \"$artifactbucketname_{region_underscores}\" --create-bucket-configuration LocationConstraint={region}; fi");
    let save_bucket_name_cmd = format!("if [[ ! -e ./s3artifactbucket_{region_underscores}.txt ]]; then echo \"$artifactbucketname_{region_underscores}\" > ./s3artifactbucket_{region_underscores}.txt; fi");
    let get_artifact_bucket_name = format!("artifactbucketname{users_func_name}=$(< ./s3artifactbucket_{region_underscores}.txt)");
    let target = "aarch64-unknown-linux-musl"; // TODO: allow user customizing this
    let compilecmd = format!("CARGO_WASMTYPEGEN_FILEOPS=\"0\" RUSTFLAGS=\"--cfg {users_func_name}\" cross rustc --crate-type=bin --release --target {target} --target-dir {target_dir}");
    let copycmd = format!("cp ./{target_dir}/{target}/release/{crate_name} ./bootstrap");
    let md5cmd = format!("md5{users_func_name}=($(md5sum ./bootstrap))");
    let zipcmd = format!("zip -r {users_func_name}_$md5{users_func_name}.zip bootstrap");
    let deployartifactcmd = format!("aws s3 cp {users_func_name}_$md5{users_func_name}.zip \"s3://$artifactbucketname{users_func_name}/\"");
    let deploycfncmd = format!("AWS_REGION=\"{region}\" aws --region {region} cloudformation deploy --stack-name hira-gen-stack --template-file deploy.yml --capabilities CAPABILITY_NAMED_IAM --parameter-overrides DefaultParam=hira ");

    let param1 = format!("{bucket_param}=$artifactbucketname{users_func_name}");
    let param2 = format!("{key_param}={users_func_name}_$md5{users_func_name}.zip");

    let deploy_file = "deploy.sh";
    let cfn_file = "deploy.yml";
    let pre_build = "# 0. pre-build:";
    let build = "# 1. build:";
    let package = "# 2. package:";
    let deploy = "# 3. deploy:";

    obj.append_to_file_unique(deploy_file, pre_build, random_name_cmd);
    obj.append_to_file_unique(deploy_file, pre_build, create_deploy_bucket_cmd);
    obj.append_to_file_unique(deploy_file, pre_build, save_bucket_name_cmd);
    obj.append_to_file(deploy_file, build, compilecmd);
    obj.append_to_file(deploy_file, build, copycmd);
    obj.append_to_file(deploy_file, build, md5cmd);
    obj.append_to_file(deploy_file, build, zipcmd);
    obj.append_to_file(deploy_file, package, get_artifact_bucket_name.into());
    obj.append_to_file(deploy_file, package, deployartifactcmd);
    obj.append_to_line(deploy_file, deploy, deploycfncmd, format!("{param1} {param2} "));

    obj.append_to_file_unique(cfn_file, "# 0", "AWSTemplateFormatVersion: '2010-09-09'".into());
    obj.append_to_file_unique(cfn_file, "# 0", "Parameters:".into());
    obj.append_to_file_unique(cfn_file, "# 1", format!("    DefaultParam:\n        Type: String"));
    obj.append_to_file(cfn_file, "# 1", format!("    {bucket_param}:\n        Type: String"));
    obj.append_to_file(cfn_file, "# 1", format!("    {key_param}:\n        Type: String"));
    obj.append_to_file_unique(cfn_file, "# 2", "Resources:".into());
    obj.append_to_file(cfn_file, "# 3", cfn_resources);
}
