fn func_macro(x) {
    if !(x.is_async()) {
        throw "Function must be async to be used as a lambda function"
    }
    let func_name = x.get_name();
    let return_type = x.get_return_type();
    if !(return_type.starts_with("Result<")) {
        throw `Return type for lambda must be a Result<_>. '${return_type}' is invalid`;
    }
    if return_type.contains(",") {
        throw `Return type for lambda must only contain one generic component. This module hijacks this Result from: Result<A> to Result<A, runtime_error>. Ensure you are only providing 1 value to the Result`;
    }
    return_type.replace("Result<", "");
    return_type.replace(">", "");
    x.set_return_type(`Result<${return_type}, lambda_runtime::Error>`);
    return_type = x.get_return_type();
    let params = x.get_parameters();
    if params.len() != 1 {
        throw `Lambda must only take 1 parameter`;
    }
    let param_type = params[0].param_type;
    let renamed_func = `actual_func_${func_name}`;
    let service_func_name = `lambda_service_func_${func_name}`;
    x.rename(renamed_func);
    let make_client_func_def = `
    // TODO: save the client somehow. dont re-create for each request...
    pub async fn make_lambda_client() -> aws_sdk_lambda::Client {
        let region = "${HIRA_DEPLOY_REGION}";
        let region_provider = aws_config::meta::region::RegionProviderChain::default_provider().or_else(region);
        let sdk_config = aws_config::from_env().region(region_provider).load().await;
        aws_sdk_lambda::Client::new(&sdk_config)
    }`;
    let service_func_def = `
    #[allow(dead_code)]
    async fn ${service_func_name}(event: lambda_runtime::LambdaEvent<${param_type}>) -> ${return_type} {
        let (x, _context) = event.into_parts();
        ${renamed_func}(x).await
    }`;
    let main_func_def = `
    #[cfg(${func_name})]
    #[tokio::main]
    async fn main() -> Result<(), lambda_runtime::Error> {
        let func = lambda_runtime::service_fn(${service_func_name});
        lambda_runtime::run(func).await?;
        Ok(())
    }`;
    // we add this to verify if the user's parameters are valid and work
    // in a lambda context. otherwise, we'd get an error during deploy, but not during build.
    let debug_type_check_def = `
    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    async fn _hira_generated_type_check_func_${func_name}() {
        let _ = lambda_runtime::run(lambda_runtime::service_fn(${service_func_name})).await;
    }
    `;
    let invoke_func_def = `
    async fn ${func_name}(n: impl Into<${param_type}>) -> ${return_type} {
        let n_param = n.into();
        let payload = match serde_json::to_string(&n_param) {
            Ok(p) => p,
            Err(e) => return Err(e.into()),
        };
        let c = make_lambda_client().await;
        let res = c.invoke().function_name("${func_name}")
            .payload(aws_sdk_lambda::types::Blob::new(payload))
            .send().await;
        match res {
            Ok(out) => {
                if let Some(payload) = out.payload() {
                    let payload = payload.as_ref();
                    match serde_json::from_slice(&payload) {
                        Ok(s) => Ok(s),
                        Err(e) => {
                            Err(e.into())
                        }
                    }
                } else {
                    Err(lambda_runtime::Error::from("Empty response from lambda").into())
                }
            }
            Err(err) => {
                Err(err.into())
            }
        }
    }`;
    x.add_code_after(service_func_def);
    x.add_code_after(main_func_def);
    x.add_code_after(debug_type_check_def);
    x.add_code_after(make_client_func_def);
    x.add_code_after(invoke_func_def);

    let bin_name = get_bin_name();
    if bin_name.is_empty() {
        throw `Failed to find binary name. This module is meant to be used in binary crates only`;
    }
    // TODO: allow user to set target to x86 optionally
    let target = "aarch64-unknown-linux-musl";
    add_build_command(`RUSTFLAGS="--cfg ${func_name}" cross build --release --target ${target} --target-dir hira/cross-target-${func_name}`);
    add_build_command(`cp hira/cross-target-${func_name}/${target}/release/${bin_name} ./bootstrap`);
    add_build_command(`md5${func_name}=($(md5sum ./bootstrap))`);
    add_build_command(`zip -r ${func_name}_$md5${func_name}.zip bootstrap`);
    let param_name = `Param${func_name}Hash`;
    param_name.replace("_", "");
    add_param_value(param_name, `${func_name}_$md5${func_name}.zip`);
    add_build_command(`mkdir -p ./hira/out && mv ${func_name}_$md5${func_name}.zip ./hira/out/`);
    add_build_command(`rm bootstrap`);

    let build_bucket = get_build_bucket();
    build_resource(x, HIRA_MOD_INPUT, func_name, build_bucket, param_name);

    x
}

fn build_resource(x, HIRA_MOD_INPUT, func_name, build_bucket, hash_param_name) {
    let resource_name = func_name;
    resource_name.replace("_", "");
    let param = "${" + hash_param_name + "}";
    let memory_size = "128";
    if HIRA_MOD_INPUT.contains("memory_size") {
        memory_size = HIRA_MOD_INPUT.memory_size;
    }
    let timeout = "30";
    if HIRA_MOD_INPUT.contains("timeout") {
        timeout = HIRA_MOD_INPUT.timeout;
    }
    let role_resource = `
    LambdaRole${resource_name}:
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
            - 'arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole'`;
    if HIRA_MOD_INPUT.contains("policy_statements") {
        let statements = HIRA_MOD_INPUT.policy_statements;
        role_resource = `${role_resource}
            Policies:
            - PolicyName: hira_generated_policy
              PolicyDocument:
                  Version: '2012-10-17'
                  Statement:`;
        for statement in statements {
            let effect = "Allow";
            if statement.contains("effect") {
                effect = statement.effect;
            }
            let action = statement.action;
            let resource = statement.resource;
            role_resource = `${role_resource}
                    - Effect: ${effect}
                      Action: ${action}
                      Resource: ${resource}`;
        }
    }
    let func_resource = `
    Lambda${resource_name}:
        Type: 'AWS::Lambda::Function'
        Properties:
            FunctionName: ${func_name}
            Runtime: provided.al2
            Code:
                S3Bucket: ${build_bucket}
                S3Key: !Sub '${param}'
            Handler: index.handler
            MemorySize: ${memory_size}
            Timeout: ${timeout}
            Architectures:
            - arm64
            Role: !GetAtt LambdaRole${resource_name}.Arn`;
    if HIRA_MOD_INPUT.contains("env_vars") {
        func_resource = `${func_resource}
            Environment:
                Variables:`;
        for env_name in HIRA_MOD_INPUT.env_vars.keys() {
            let env_value = HIRA_MOD_INPUT.env_vars[env_name];
            func_resource = `${func_resource}
                    ${env_name}: ${env_value}`;
        }
    }

    if HIRA_MOD_INPUT.contains("triggers") {
        let i = 0;
        for trigger in HIRA_MOD_INPUT.triggers {
            i += 1;
            if trigger.type == "function_url" {
                x.set_global_const("FUNCTION_URL", `!Select [2, !Split ["/", !GetAtt LambdaTrigger${resource_name}${i}.FunctionUrl]]`);
                let function_url = `
    LambdaTrigger${resource_name}${i}:
        Type: 'AWS::Lambda::Url'
        Properties:
            AuthType: NONE
            TargetFunctionArn: !GetAtt Lambda${resource_name}.Arn`;
                if trigger.contains("cors") {
                    function_url = `${function_url}
            Cors:
                    `;
                    let allow_creds = "false";
                    let max_age = "0";
                    let allow_methods = `["*"]`;
                    let allow_origins = `["*"]`;
                    let cors = trigger.cors;
                    if cors.contains("allow_creds") {
                        allow_creds = cors.allow_creds;
                    }
                    if cors.contains("max_age") {
                        max_age = cors.max_age;
                    }
                    if cors.contains("allow_methods") {
                        allow_methods = cors.allow_methods;
                    }
                    if cors.contains("allow_origins") {
                        allow_origins = cors.allow_origins;
                    }
                    function_url = `${function_url}
                AllowCredentials: ${allow_creds}
                MaxAge: ${max_age}
                AllowMethods: ${allow_methods}
                AllowOrigins: ${allow_origins}`;
                }
                let function_permission = `
    LambdaPermission${resource_name}${i}:
        Type: AWS::Lambda::Permission
        Properties:
            Action: 'lambda:InvokeFunctionUrl'
            FunctionName: !GetAtt Lambda${resource_name}.Arn
            FunctionUrlAuthType: NONE
            Principal: '*'`;
                add_to_cfn(function_permission);
                add_to_cfn(function_url);
            } else {
                throw `Unknown lambda trigger ${trigger}`;
            }
        }
    }

    add_to_cfn(func_resource);
    add_to_cfn(role_resource);
}
