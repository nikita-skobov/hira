
mod parsing;
use std::{env, io::Write, process::Command, str::FromStr};

use parsing::*;

mod resources;
use resources::*;


#[proc_macro_attribute]
pub fn create_lambda(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = parse_attributes(attr);
    let lambda_conf: LambdaFunction = attr.into();

    let mut bin_name = "".to_string();
    for (key, value) in env::vars() {
        if key == "RUSTFLAGS" {
            println!("RUSTFLAGS: {:?}", value);
        }
        if key == "CARGO_BIN_NAME" {
            bin_name = value;
        }
    }
    if bin_name.is_empty() {
        panic!("Must build this in a binary crate. failed to find CARGO_BIN_NAME");
    }

    // println!("ITEM: {:#?}", item);
    let mut func_def = parse_func_def(item, false);
    func_def.assert_num_params(1);
    let ret_type = func_def.get_return_type();
    let func_name = func_def.get_func_name();
    let use_async = if func_def.fn_async_ident.is_some() {
        ".await"
    } else {
        ""
    };
    let (_, use_type) = func_def.get_nth_param(0);
    let (use_ret, use_body) = if ret_type.starts_with("Result") {
        (ret_type, format!("let (x, _context) = event.into_parts(); {func_name}(x){use_async}"))
    } else {
        // if it's empty, use the default Result<(), Error>
        if ret_type.is_empty() {
            ("Result<(), Error>".into(), format!("Ok({func_name}(){use_async})"))
        } else {
            (format!("Result<{}, Error>", ret_type), format!("let (x, _context) = event.into_parts(); Ok({func_name}(x){use_async})"))
        }
    };

    let main_str = format!("
        #[cfg({func_name})]
        #[tokio::main]
        async fn main() -> Result<(), Error> {{
            let func = service_fn(lambda_service_func);
            lambda_runtime::run(func).await?;
            Ok(())
        }}
    ");
    let prototype_str = format!("
        #[cfg({func_name})]
        async fn lambda_service_func(event: LambdaEvent<{use_type}>) -> {use_ret} {{ {use_body} }}"
    );
    let main_stream = TokenStream::from_str(&main_str).unwrap();
    let prototype_stream = TokenStream::from_str(&prototype_str).unwrap();
    let mut out = func_def.build();
    out.extend(prototype_stream);
    out.extend(main_stream);

    // TODO: allow user to set target to x86 optionally
    let target = "aarch64-unknown-linux-musl";
    add_build_cmd(format!("RUSTFLAGS=\"--cfg {func_name}\" cross build --release --target {target}"));
    add_build_cmd(format!("cp target/{target}/release/{bin_name} ./bootstrap"));
    add_build_cmd(format!("zip -r {func_name}.zip bootstrap"));
    add_build_cmd(format!("mkdir -p ./out && mv {func_name}.zip ./out/"));
    add_build_cmd(format!("rm bootstrap"));
    let build_bucket = unsafe {&BUILD_BUCKET};
    if build_bucket.is_empty() {
        panic!("No build bucket found. Must provide a bucket name via set_build_bucket!();");
    }
    add_lambda_resource(build_bucket, &func_name, lambda_conf);

    println!("{}", out.to_string());
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
    file.write_all("rm -rf ./out/\n".as_bytes()).expect("failed to write");
    for step in BUILD_COMMANDS.iter() {
        file.write_all(step.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.write_all("\n# package:\n".as_bytes()).expect("failed to write");
    let bucket = unsafe {&BUILD_BUCKET};
    file.write_all(format!("aws s3 cp --recursive ./out/ s3://{bucket}").as_bytes()).expect("failed to write");
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
    file.write_all(format!("AWS_REGION={region} aws --region {region} cloudformation deploy --stack-name {stack_name} --template-file ./deploy.yml --capabilities CAPABILITY_NAMED_IAM").as_bytes()).expect("Failed to write");
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
    file.write_all("Resources:\n".as_bytes()).expect("Failed to write");
    for resource in RESOURCES.iter() {
        file.write_all(resource.as_bytes()).expect("failed to write");
        file.write_all("\n".as_bytes()).expect("failed to write");
    }
    file.flush().expect("Failed to finish writing to file");
}
