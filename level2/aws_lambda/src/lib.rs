use std::io::Write;
use aws_sdk_s3::primitives::ByteStream;
use cfn_resources::CfnResource;
use hira_lib::level0::*;
use hira_lib::parsing::FunctionSignature;
use aws_cfn_stack::aws_cfn_stack;
use ::aws_cfn_stack::{aws_cfn_stack::{SavedResource, SavedTemplate, ResourceOutput}, create_or_update_stack, wait_for_output};
use cfn_resources::serde_json::Value;
use tokio::io::AsyncReadExt;
use zip::write::FileOptions;
use zip::result::ZipResult;
use zip::write::ZipWriter;
use std::io::Cursor;


pub fn get_ref(logical_name: &str) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("Ref".to_string(), serde_json::Value::String(logical_name.to_string()));
    serde_json::Value::Object(map)
}

pub async fn create_bucket_stack() -> String {
    const STACK_NAME: &str = "hira-gen-lambda-artifact-bucket";
    let sdk_config = aws_config::from_env().load().await;
    let client = aws_sdk_cloudformation::Client::new(&sdk_config);

    // check if this stack already exists.
    // if it does, then just return the name of the s3 bucket output
    match wait_for_output(&client, STACK_NAME, None).await {
        Ok(o) => match o.get("BucketName") {
            Some(s) => return s.to_string(),
            None => panic!("Stack {} already exists, but is missing a BucketName output", STACK_NAME),
        },
        Err(_) => {
            // assume it doesnt exist, try to create it:
        }
    }

    let s3_bucket = s3::bucket::CfnBucket {
        ..Default::default()
    };
    let resource = SavedResource {
        ty: s3_bucket.type_string().to_string(),
        properties: s3_bucket.properties(),
    };
    let mut template = SavedTemplate::default();
    template.resources.insert("S3ArtifactBucket".to_string(), resource);
    template.outputs.insert("BucketName".to_string(), ResourceOutput {
        description: "name of bucket created".to_string(),
        value: get_ref("S3ArtifactBucket"),
    });
    let template_body = cfn_resources::serde_json::to_string(&template).expect("Failed to serialize template");
    if let Err(e) = create_or_update_stack(&client, STACK_NAME, &template_body).await {
        panic!("Failed to create {STACK_NAME} stack\n{e}");
    }
    let outputs = match wait_for_output(&client, STACK_NAME, None).await {
        Ok(o) => o,
        Err(e) => panic!("Failed to get outputs for {STACK_NAME}\n{}", e),
    };
    match outputs.get("BucketName") {
        Some(s) => s.to_string(),
        None => panic!("Failed to get BucketName output for {STACK_NAME}"),
    }
}

pub async fn set_bucket_arn(bucket_name: &mut String, bucket_location: &mut Option<String>) {
    if let Some(location) = bucket_location {
        *bucket_name = location.clone();
    } else {
        let location = create_bucket_stack().await;
        *bucket_location = Some(location.clone());
        *bucket_name = location;
    }
}

fn create_zip_archive(data: &[u8]) -> ZipResult<Vec<u8>> {
    let mut zip_buffer = Cursor::new(Vec::new());
    let mut zip_writer = ZipWriter::new(&mut zip_buffer);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    zip_writer.start_file("bootstrap", options)?;
    zip_writer.write_all(data)?;
    zip_writer.finish()?;
    drop(zip_writer);

    // Get the written zip data from the buffer
    let zip_data = zip_buffer.into_inner();
    Ok(zip_data)
}

pub fn basic_hash(data: &[u8]) -> String {
    let hash = adler::adler32(data).unwrap_or(0);
    format!("{:X}", hash)
}

pub async fn zip_and_upload_lambda_code(src_path: &str, dest_bucket: &str) -> String {
    let mut file_data = vec![];
    match tokio::fs::File::open(&src_path).await {
        Ok(mut f) => match f.read_to_end(&mut file_data).await {
            Ok(_) => {}
            Err(e) => panic!("Failed to read artifact file {src_path}\n{:?}", e),
        },
        Err(e) => panic!("Failed to read artifact file {src_path}\n{:?}", e),
    }
    let hash_str = basic_hash(&file_data);
    let zipped_data = match create_zip_archive(&file_data) {
        Ok(d) => d,
        Err(e) => panic!("Failed to create zip archive for {src_path}\n{:?}", e),
    };
    let base_name = match src_path.rsplit_once("/") {
        Some((_, right)) => right.to_string(),
        None => "lambdafn".to_string()
    };
    let obj_key = format!("{base_name}-{hash_str}.zip");

    let sdk_config = aws_config::from_env().load().await;
    let client = aws_sdk_s3::Client::new(&sdk_config);

    let resp = client.put_object()
        .bucket(dest_bucket)
        .key(&obj_key)
        .body(ByteStream::from(zipped_data))
        .send().await;
    if let Err(e) = resp {
        panic!("Failed to upload {src_path} to s3://{dest_bucket}\n{:?}", e);
    }
    obj_key
}

pub async fn setup_lambda(data: &mut Vec<String>) {
    use crate::h_aws_lambda::BUCKET_UNKNOWN;
    let bucket_location = create_bucket_stack().await;
    let bucket_location_ref = bucket_location.as_str();

    println!("Uploading Lambdas Function Artifacts...");
    for stack_str in data {
        let mut stack: aws_cfn_stack::SavedStack = cfn_resources::serde_json::from_str(&stack_str).expect("Failed to deserialize generated json file");
        for (_stack_name, (_, template)) in stack.template.iter_mut() {
            for (resource_name, resource) in template.resources.iter_mut() {
                if let Some((mut bucket_name, mut obj_key)) = get_function_code_location(resource) {
                    // this lambda function doesnt have a bucket name yet, so we set it
                    if bucket_name == BUCKET_UNKNOWN {
                        bucket_name = bucket_location_ref.to_string();
                    }
                    // upload the file to the bucket location:
                    println!("Zipping and uploading artifact for {resource_name}");
                    obj_key = zip_and_upload_lambda_code(&obj_key, bucket_location_ref).await;
                    reinsert(resource, bucket_name, obj_key);
                }
            }
        }
        // serialize it back and store in the string
        *stack_str = cfn_resources::serde_json::to_string(&stack).expect("Failed to serialize generated json");
    }
}

pub fn reinsert(resource: &mut SavedResource, bucket_name: String, obj_key: String) {
    if let Some(Value::Object(code)) = resource.properties.get_mut("Code") {
        code.insert("S3Bucket".to_string(), Value::String(bucket_name));
        code.insert("S3Key".to_string(), Value::String(obj_key));
    }
}

/// given a SavedResource, return an option that contains
/// the bucket name, and object key.
/// none if the resource is not a function.
/// it is up to the caller to re-insert the modified
/// values back into the saved resource via calling `reinsert`
pub fn get_function_code_location(resource: &SavedResource) -> Option<(String, String)> {
    let cfn_lambda = lambda::function::CfnFunction::default();
    if resource.ty != cfn_lambda.type_string() {
        return None;
    }
    let code_obj = match resource.properties.get("Code") {
        Some(Value::Object(o)) => o,
        _ => return None,
    };
    match (code_obj.get("S3Bucket"), code_obj.get("S3Key")) {
        (Some(Value::String(bucket)), Some(Value::String(key))) => {
            Some((bucket.to_string(), key.to_string()))
        }
        _ => None,
    }
}

/// This is a higher level module for easily creating lambda functions.
/// To use this module, your calling module must contain a function `lambda_main`.
/// This module parses the function signature of `lambda_main` and generates corresponding
/// runtime and cloudformation resource(s). By default the runtime is arm64, and we create
/// a role for your function automatically. To customize these, and other behaviors, see
/// the input section.
#[hira::hira]
pub mod h_aws_lambda {
    extern crate lambda;
    extern crate iam;
    extern crate cfn_resources;
    use super::FunctionSignature;
    use super::aws_cfn_stack;
    use self::aws_cfn_stack::ResourceOutput;
    use super::L0RuntimeCreator;
    use super::L0CodeWriter;
    use super::L0CodeReader;
    use super::L0Core;
    use super::RuntimeMeta;
    use self::cfn_resources::get_att;
    use self::cfn_resources::ToOptStrVal;
    use self::cfn_resources::serde_json::Value;
    use self::cfn_resources::StrVal;

    pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

    pub const BUCKET_UNKNOWN: &str = "HIRA_GEN_BUCKET_UNKNOWN";

    #[derive(cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[serde(default)]
    #[derive(Default)]
    pub struct FunctionUrlEvent {
        pub version: String,
        pub body: String,
        pub is_base64_encoded: bool,
    }

    #[derive(cfn_resources::serde::Serialize, cfn_resources::serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FunctionUrlResponse {
        pub status_code: u32,
        pub body: String,
        pub is_base64_encoded: bool,
        pub headers: std::collections::HashMap<String, String>,
    }

    /// statements contain a tuple of: effect, action, resource.
    /// eg: ("Allow", "*", "*")
    pub fn create_policy_doc(statements: &[(String, String, String)]) -> Value {
        let mut map = cfn_resources::serde_json::Map::default();
        map.insert("Version".to_string(), Value::String("2012-10-17".to_string()));
        let mut statements_out = vec![];
        for (effect, action, resource) in statements {
            let mut statement_obj = cfn_resources::serde_json::Map::default();
            statement_obj.insert("Effect".to_string(), Value::String(effect.to_string()));
            statement_obj.insert("Action".to_string(), Value::String(action.to_string()));
            statement_obj.insert("Resource".to_string(), Value::String(resource.to_string()));
            statements_out.push(Value::Object(statement_obj));
        }
        map.insert("Statement".to_string(), Value::Array(statements_out));
        Value::Object(map)
    }

    pub fn create_assume_role_policy_doc() -> Value {
        let mut map = cfn_resources::serde_json::Map::default();
        map.insert("Version".to_string(), Value::String("2012-10-17".to_string()));

        let mut principal = cfn_resources::serde_json::Map::default();
        principal.insert("Service".to_string(), Value::String("lambda.amazonaws.com".to_string()));

        let mut statements_out = vec![];
        let mut statement_obj = cfn_resources::serde_json::Map::default();
        statement_obj.insert("Effect".to_string(), Value::String("Allow".to_string()));
        statement_obj.insert("Principal".to_string(), Value::Object(principal));
        statement_obj.insert("Action".to_string(), Value::String("sts:AssumeRole".to_string()));
        statements_out.push(Value::Object(statement_obj));
        map.insert("Statement".to_string(), Value::Array(statements_out));
        Value::Object(map)
    }

    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub enum Arch {
        Arm64,
        X86,
    }

    impl Default for Arch {
        fn default() -> Self { Self::Arm64 }
    }
    impl Arch {
        pub fn to_string(&self) -> String {
            match self {
                Arch::Arm64 => "aarch64-unknown-linux-musl".to_string(),
                Arch::X86 => "x86_64-unknown-linux-musl".to_string(),
            }
        }
    }

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct Input {
        /// by default, we add policy statements to the lambda's execution role to allow
        /// it to log to cloudwatch. if you'd like to disable cloudwatch logging, set
        /// this to true.
        pub disable_cloudwatch_logging: bool,
        /// optionally add extra policy statements. this is a list of tuples
        /// where the tuple is (Effect, Action, Resource)
        /// for example ("Allow", "logs:CreateLogStream", "*")
        pub extra_policy_statements: Vec<(String, String, String)>,
        /// by default, we will create a role with all of the permissions
        /// defined in `extra_policies` + default cloudwatch policies.
        /// if you specify a role_arn, we only use the provided ARN.
        pub role_arn: String,

        /// by default we try to determine if you wish to use a Lambda FunctionURL
        /// by looking at the signature of your lambda_main function.
        /// If you'd like to explicitly use a Lambda FunctionURL without
        /// relying on code parsing, you can set this to true.
        /// Note: setting this to false has no effect.
        pub use_function_url: bool,

        /// valid values: arm64, x86. Defaults to arm64
        /// This controls how the lambda function will be compiled.
        /// arm64: aarch64-unknown-linux-musl
        /// x86: x86_64-unknown-linux-musl
        pub architecture: Arch,

        /// This module only sets the following fields:
        /// - architectures
        /// - code
        /// - handler
        /// - role
        /// - runtime
        /// if you desire to customize your lambda function further
        /// you can set extra_options. for example:
        /// ```rust,ignore
        /// extra_options.memory_size = Some(1024);
        /// ```
        pub extra_options: lambda::function::CfnFunction,
    }

    pub mod outputs {
        /// the logical id this function has in cloudformation.
        /// you can use this output in other modules to reference this function
        pub const LOGICAL_FUNCTION_NAME: &str = "UNDEFINED";
        /// the logical id of the function url resource (if created)
        pub const LOGICAL_FUNCTION_URL_NAME: &str = "UNDEFINED";
    }

    #[hira::hiracfg(editor)]
    /// example fn
    pub fn lambda_main(_something: FunctionUrlEvent) -> FunctionUrlResponse {
        todo!()
    }


    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("RUNTIME", &[""]),
        ("CODE_WRITE", &["fn_module:service_func", "fn_module:entrypoint"]),
        ("CODE_READ", &["fn:lambda_main"]),
    ];

    fn validate_lambda_main_signature(sig: &FunctionSignature, use_event_func_url: &mut bool) -> Result<(String, String, String), String> {
        if sig.return_ty.is_empty() {
            return Err("Must provide a return type for your lambda_main function".into());
        }
        let return_ty = sig.return_ty.replace(" ", "");
        let (wrap_with_ok, b) = if return_ty.starts_with("Result<") {
            if !return_ty.ends_with("BoxError>") {
                return Err(format!("Your function uses an invalid result type '{return_ty}'. If returning a Result<> the error type must be '::aws_lambda::h_aws_lambda::BoxError'"));
            }
            (false, return_ty)
        } else {
            (true, format!("Result<{return_ty}, ::aws_lambda::h_aws_lambda::BoxError>"))
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
        if input_param_type.ends_with("FunctionUrlEvent") {
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
        inp: &mut Input, stackinp: &mut aws_cfn_stack::Input, l0code: &mut L0CodeReader,
        runtimer: &mut L0RuntimeCreator, l0core: &mut L0Core, l0write: &mut L0CodeWriter
    ) {
        let user_mod_name = l0core.users_module_name();
        let lambda_main_signature = match l0code.get_fn("lambda_main") {
            Some(sig) => sig,
            None => {
                l0core.compiler_error("Missing lambda_main function");
                return;
            }
        };
        let (return_statement, return_ty, input_param_type) = match validate_lambda_main_signature(lambda_main_signature, &mut inp.use_function_url) {
            Err(e) => {
                l0core.compiler_error(&e);
                return;
            }
            Ok(p) => p,
        };

        // Box<dyn std::error::Error + Send + Sync + 'static> = BoxError
        l0write.write_internal_fn(
            format!("pub async fn service_func(event: lambda_runtime::LambdaEvent<{}>) -> {}", input_param_type, return_ty),
            format!("let (x, _context) = event.into_parts(); {}", return_statement)
        );
        l0write.write_internal_fn(
            format!("pub async fn entrypoint() -> Result<(), ::aws_lambda::h_aws_lambda::BoxError>"),
            format!("let func = lambda_runtime::service_fn(service_func);\nlambda_runtime::run(func).await?;\nOk(())")
        );
        runtimer.add_to_runtime_ex(
            &user_mod_name,
            format!("{user_mod_name}::entrypoint().await.expect(\"Lambda Error\")"),
            RuntimeMeta { cargo_cmd: "cross".to_string(), target: inp.architecture.to_string(), profile: "release".to_string() }
        );
        runtimer.depends_on(&user_mod_name, "deploy");
        let lambda_executable_path = runtimer.get_full_runtime_path(&user_mod_name);

        let mut default_statements = vec![
            ("Allow".to_string(), "logs:CreateLogGroup".to_string(), "*".to_string()),
            ("Allow".to_string(), "logs:CreateLogStream".to_string(), "*".to_string()),
            ("Allow".to_string(), "logs:PutLogEvents".to_string(), "*".to_string()),
        ];
        if inp.disable_cloudwatch_logging {
            default_statements.clear();
        }
        default_statements.extend(inp.extra_policy_statements.clone());

        let policy = iam::role::Policy {
            policy_name: format!("hira-gen-policy-{user_mod_name}").into(),
            policy_document: create_policy_doc(&default_statements),
        };
        let role_name = format!("hira-gen-{user_mod_name}-role");
        let logical_role_name = role_name.replace("-", "");
        let logical_role_name = logical_role_name.replace("_", "");
        let logical_fn_name = format!("hiragen{user_mod_name}");
        let logical_fn_name = logical_fn_name.replace("_", "");
        let role = iam::role::CfnRole {
            description: Some(format!("auto generated for {user_mod_name}").into()),
            assume_role_policy_document: create_assume_role_policy_doc(),
            role_name: Some(role_name.clone().into()),
            policies: Some(vec![policy]),
            ..Default::default()
        };
        let extra_options = std::mem::take(&mut inp.extra_options);

        let lambdafn = lambda::function::CfnFunction {
            architectures: Some(vec![
                match inp.architecture {
                    Arch::X86 => "x86_64".to_string(),
                    Arch::Arm64 => "arm64".to_string(),
                }
            ]),
            code: lambda::function::Code {
                s3_bucket: BUCKET_UNKNOWN.to_str_val(),
                s3_key: Some(lambda_executable_path.into()),
                ..Default::default()
            },
            handler: Some("index.handler".into()),
            role: if inp.role_arn.is_empty() {
                StrVal::Val(cfn_resources::get_att(&logical_role_name, "Arn"))
            } else {
                inp.role_arn.clone().into()
            },
            runtime: Some(lambda::function::FunctionRuntimeEnum::Providedal2),
            ..extra_options
        };
        l0core.set_output("LOGICAL_FUNCTION_NAME", &logical_fn_name);

        let resource = aws_cfn_stack::Resource {
            name: logical_fn_name.clone(),
            properties: Box::new(lambdafn) as _,
        };
        let role_resource = aws_cfn_stack::Resource {
            name: logical_role_name.to_string(),
            properties: Box::new(role) as _,
        };
        stackinp.run_before.push("::aws_lambda::setup_lambda(&mut runtime_data).await".to_string());
        stackinp.resources.push(resource);
        if inp.role_arn.is_empty() {
            stackinp.resources.push(role_resource);
        }
        let arn_output_name = format!("LambdaFunctionArn{}", user_mod_name);
        let arn_output_name = arn_output_name.replace("_", "");
        let resource_out = ResourceOutput {
            description: "".to_string(),
            value: get_att(&logical_fn_name, "Arn")
        };
        stackinp.outputs.insert(arn_output_name, resource_out);

        if inp.use_function_url {
            let func_url = lambda::url::CfnUrl {
                auth_type: lambda::url::UrlAuthTypeEnum::None,
                target_function_arn: StrVal::Val(cfn_resources::get_att(&logical_fn_name, "Arn")),
                ..Default::default()
            };
            let func_permission = lambda::permission::CfnPermission {
                action: "lambda:InvokeFunctionUrl".into(),
                function_name: StrVal::Val(cfn_resources::get_att(&logical_fn_name, "Arn")),
                function_url_auth_type: Some(lambda::permission::PermissionFunctionUrlAuthTypeEnum::None),
                principal: "*".into(),
                ..Default::default()
            };
            let logical_url_name = format!("hiragen{user_mod_name}url");
            let logical_url_name = logical_url_name.replace("_", "");
            let logical_permission_name = format!("{}permission", logical_url_name);
            let url_resource = aws_cfn_stack::Resource {
                name: logical_url_name.clone().to_string(),
                properties: Box::new(func_url) as _,
            };
            let permission_resource = aws_cfn_stack::Resource {
                name: logical_permission_name.to_string(),
                properties: Box::new(func_permission) as _,
            };
            stackinp.resources.push(permission_resource);
            stackinp.resources.push(url_resource);

            let arn_output_name = format!("LambdaFunctionUrl{}", user_mod_name);
            let arn_output_name = arn_output_name.replace("_", "");
            let resource_out = ResourceOutput {
                description: "".to_string(),
                value: get_att(&logical_url_name, "FunctionUrl")
            };
            stackinp.outputs.insert(arn_output_name, resource_out);

            l0core.set_output("LOGICAL_FUNCTION_URL_NAME", &logical_url_name);
        }
    }
}
