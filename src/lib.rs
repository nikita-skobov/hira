
mod parsing;
use std::{env, io::Write, process::Command, str::FromStr};

use parsing::*;

mod resources;
use resources::*;

#[proc_macro_attribute]
pub fn create_s3(attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: handle parsing the module under the item, and add convenience functions
    // to the module
    let attr = parse_attributes(attr);
    let s3_conf: S3Bucket = attr.into();
    add_s3_bucket_resource(s3_conf);
    item
}

#[proc_macro_attribute]
pub fn create_cloudfront_distribution(attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: handle parsing the module under the item, and add convenience functions
    // to the module
    let attr = parse_attributes(attr);
    let conf: CloudfrontDistribution = attr.into();
    // TODO: if the conf doesnt have a name/description, set it
    // via the mod name item
    add_cloudfront_resource(conf);
    item
}

#[proc_macro_attribute]
pub fn create_route53_record(attr: TokenStream, item: TokenStream) -> TokenStream {
    // TODO: handle parsing the module under the item, and add convenience functions
    // to the module
    let attr = parse_attributes(attr);
    let conf: Route53RecordSet = attr.into();
    add_route53_resource(conf);
    item
}

#[proc_macro_attribute]
pub fn create_lambda(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = parse_attributes(attr);
    let lambda_conf: LambdaFunction = attr.into();

    let mut should_output_client = false;
    unsafe {
        if !CREATED_LAMBDA {
            CREATED_LAMBDA = true;
            should_output_client = true;
        }
    }

    let mut bin_name = "".to_string();
    for (key, value) in env::vars() {
        if key == "CARGO_BIN_NAME" || key == "CARGO_CRATE_NAME" {
            bin_name = value;
        }
    }
    if bin_name.is_empty() {
        panic!("Must build this in a binary crate. failed to find CARGO_BIN_NAME");
    }

    // println!("ITEM: {:#?}", item);
    let mut func_def = parse_func_def(item, false);
    func_def.assert_num_params(1);
    if func_def.fn_async_ident.is_none() {
        panic!("Lambda functions must be async");
    }
    let ret_type = func_def.get_return_type();
    let func_name = func_def.get_func_name();
    let use_async = if func_def.fn_async_ident.is_some() {
        ".await"
    } else {
        ""
    };
    let renamed_func = format!("actual_{func_name}");
    func_def.change_func_name(&renamed_func);
    let (_, use_type) = func_def.get_nth_param(0);
    let (use_ret, use_body) = if ret_type.starts_with("Result") {
        (ret_type.clone(), format!("let (x, _context) = event.into_parts(); {renamed_func}(x){use_async}"))
    } else {
        // if it's empty, use the default Result<(), Error>
        if ret_type.is_empty() {
            ("Result<(), lambda_runtime::Error>".into(), format!("Ok({renamed_func}(){use_async})"))
        } else {
            (format!("Result<{}, lambda_runtime::Error>", ret_type.clone()), format!("let (x, _context) = event.into_parts(); Ok({renamed_func}(x){use_async})"))
        }
    };

    let region = unsafe {&DEPLOY_REGION};
    let client_func_str = &format!("
        // TODO: save the client somehow. dont re-create for each request...
        pub async fn make_lambda_client() -> aws_sdk_lambda::Client {{
            let region_provider = aws_config::meta::region::RegionProviderChain::default_provider().or_else({region});
            let sdk_config = aws_config::from_env().region(region_provider).load().await;
            aws_sdk_lambda::Client::new(&sdk_config)
        }}
    ");

    let invoke_safe_str = format!("
        async fn {func_name}_safe(n: {use_type}) -> Result<{ret_type}, lambda_runtime::Error> {{
            let payload = match serde_json::to_string(&n) {{
                Ok(p) => p,
                Err(e) => return Err(e.into()),
            }};
            let c = make_lambda_client().await;
            let res = c.invoke().function_name(\"{func_name}\")
                .payload(aws_sdk_lambda::types::Blob::new(payload))
                .send().await;
            match res {{
                Ok(out) => {{
                    if let Some(payload) = out.payload() {{
                        let payload = payload.as_ref();
                        match serde_json::from_slice::<{ret_type}>(&payload) {{
                            Ok(s) => Ok(s),
                            Err(e) => {{
                                Err(lambda_runtime::Error::from(e))
                            }}
                        }}
                    }} else {{
                        Err(lambda_runtime::Error::from(\"Empty response from lambda\"))
                    }}
                }}
                Err(err) => {{
                    Err(err.into())
                }}
            }}
        }}
    ");
    let invoke_str = format!("
        async fn {func_name}(n: {use_type}) -> {ret_type} {{
            match {func_name}_safe(n).await {{
                Ok(r) => r,
                Err(e) => {{
                    println!(\"Failed to invoke {func_name}: {{:?}}\", e);
                    // purposefully hide errors from being returned from lambda.
                    // the real errors can be found in cloudwatch.
                    panic!(\"internal error\");
                }}
            }}
        }}
    ");

    let main_str = format!("
        #[cfg({func_name})]
        #[tokio::main]
        async fn main() -> Result<(), lambda_runtime::Error> {{
            let func = lambda_runtime::service_fn(lambda_service_func);
            lambda_runtime::run(func).await?;
            Ok(())
        }}
    ");
    let prototype_str = format!("
        #[cfg({func_name})]
        async fn lambda_service_func(event: lambda_runtime::LambdaEvent<{use_type}>) -> {use_ret} {{ {use_body} }}"
    );
    let client_func_stream = TokenStream::from_str(&client_func_str).unwrap();
    let main_stream = TokenStream::from_str(&main_str).unwrap();
    let invoke_safe_stream = TokenStream::from_str(&invoke_safe_str).unwrap();
    let invoke_stream = TokenStream::from_str(&invoke_str).unwrap();
    let prototype_stream = TokenStream::from_str(&prototype_str).unwrap();
    let dont_warn_stream = TokenStream::from_str("#[allow(dead_code)]").unwrap();
    let mut out = dont_warn_stream;
    out.extend(func_def.build());
    out.extend(prototype_stream);
    out.extend(main_stream);
    if should_output_client {
        out.extend(client_func_stream);
    }
    out.extend(invoke_safe_stream);
    out.extend(invoke_stream);

    // TODO: allow user to set target to x86 optionally
    let target = "aarch64-unknown-linux-musl";
    add_build_cmd(format!("RUSTFLAGS=\"--cfg {func_name}\" cross build --release --target {target} --target-dir hira/cross-target-{func_name}"));
    add_build_cmd(format!("cp hira/cross-target-{func_name}/{target}/release/{bin_name} ./bootstrap"));
    add_build_cmd(format!("md5{func_name}=($(md5sum ./bootstrap))"));
    add_build_cmd(format!("zip -r {func_name}_$md5{func_name}.zip bootstrap"));
    let mut param_name = format!("Param{func_name}Hash");
    param_name = param_name.replace("_", "");
    add_param_value((&param_name, format!("{func_name}_$md5{func_name}.zip")));
    add_build_cmd(format!("mkdir -p ./hira/out && mv {func_name}_$md5{func_name}.zip ./hira/out/"));
    add_build_cmd(format!("rm bootstrap"));
    let build_bucket = unsafe {&BUILD_BUCKET};
    if build_bucket.is_empty() {
        panic!("No build bucket found. Must provide a bucket name via set_build_bucket!();");
    }
    add_lambda_resource(build_bucket, &func_name, lambda_conf, param_name);
    out
}


#[proc_macro]
pub fn close(_item: TokenStream) -> TokenStream {
    let var = env::var("RUSTFLAGS").ok();
    // no rustflags means we assume this is the first pass, in
    // this case we wish to output an empty main, and we wish
    // to output the commands to a deploy.sh
    if var.is_none() {
        unsafe { output_cloudformation_yml(); }
        unsafe { output_deployment_file(); }
        return "fn main() {}".parse().unwrap()
    }

    "".parse().unwrap()
}

/// sets the S3 bucket that will be used to deploy build artifacts (if any)
#[proc_macro]
pub fn set_build_bucket(item: TokenStream) -> TokenStream {
    let mut iter = item.into_iter();
    if let proc_macro::TokenTree::Literal(s) = iter.next().expect("must provide bucket to set_build_bukcet") {
        unsafe {
            BUILD_BUCKET = s.to_string();
        }
    }
    "".parse().unwrap()
}

/// sets the region that this app will be deployed to
#[proc_macro]
pub fn set_deploy_region(item: TokenStream) -> TokenStream {
    let mut iter = item.into_iter();
    if let proc_macro::TokenTree::Literal(s) = iter.next().expect("must provide bucket to set_build_bukcet") {
        unsafe {
            DEPLOY_REGION = s.to_string();
        }
    }
    "".parse().unwrap()
}

/// sets the stack name for this app. if no stack name provided, the default is to
/// use the application name
#[proc_macro]
pub fn set_stack_name(item: TokenStream) -> TokenStream {
    let mut iter = item.into_iter();
    if let proc_macro::TokenTree::Literal(s) = iter.next().expect("must provide stack name to set_stack_name") {
        unsafe {
            STACK_NAME = s.to_string();
        }
    }
    "".parse().unwrap()
}

unsafe fn output_deployment_file() {
    let mut file = std::fs::File::create("./deploy.sh").expect("Failed to create deploy.sh file");
    file.write_all("#!/usr/bin/env bash\n\n".as_bytes()).expect("failed to write");
    file.write_all("# build:\n".as_bytes()).expect("failed to write");
    file.write_all("rm -rf ./hira/out/\n".as_bytes()).expect("failed to write");
    for step in BUILD_COMMANDS.iter() {
        file.write_all(step.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.write_all("\n# package:\n".as_bytes()).expect("failed to write");
    let bucket = unsafe {&BUILD_BUCKET};
    file.write_all(format!("aws s3 sync --size-only ./hira/out/ s3://{bucket}").as_bytes()).expect("failed to write");
    for step in PACKAGE_COMMANDS.iter() {
        file.write_all(step.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.write_all("\n# deploy:\n".as_bytes()).expect("failed to write");
    let region = unsafe {&DEPLOY_REGION};
    let mut stack_name = unsafe {STACK_NAME.clone()};
    if stack_name.is_empty() {
        stack_name = env::var("CARGO_BIN_NAME").expect("No stack name provided, and failed to use cargo bin name as stack name");
    }
    let mut cmd = format!("AWS_REGION={region} aws --region {region} cloudformation deploy --stack-name {stack_name} --template-file ./deploy.yml --capabilities CAPABILITY_NAMED_IAM");
    if !PARAMETER_VALUES.is_empty() {
        cmd.push_str(" --parameter-overrides ");
        for (key, value) in &PARAMETER_VALUES {
            cmd.push_str(&format!("{key}={value} "));
        }
    }
    file.write_all(cmd.as_bytes()).expect("Failed to write");
    for step in DEPLOY_COMMANDS.iter() {
        file.write_all(step.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.flush().expect("Failed to finish writing to file");
    if cfg!(unix) {
        let out = Command::new("chmod").arg("+x").arg("./deploy.sh").output().expect("Failed to make deploy.sh executable");
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            panic!("Failed to make deploy.sh executable: {err}");
        }
    }
}

unsafe fn output_cloudformation_yml() {
    let mut file = std::fs::File::create("./deploy.yml").expect("Failed to create deploy.yml file");
    file.write_all("AWSTemplateFormatVersion: '2010-09-09'\n".as_bytes()).expect("failed to write");
    if !PARAMETER_VALUES.is_empty() {
        file.write_all("Parameters:\n".as_bytes()).expect("failed to write");
        for p in &PARAMETER_VALUES {
            let key = &p.0;
            file.write_all(format!("    {key}:\n        Type: String\n").as_bytes()).expect("failed to write");
        }
    }
    file.write_all("Resources:\n".as_bytes()).expect("Failed to write");
    for resource in RESOURCES.iter() {
        file.write_all(resource.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.flush().expect("Failed to finish writing to file");
}
