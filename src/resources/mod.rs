pub use std::collections::HashMap;

pub use super::parsing::*;

mod lambda;
pub use lambda::*;
mod s3_bucket;
pub use s3_bucket::*;
mod cloudfront;
pub use cloudfront::*;
mod route53;
pub use route53::*;

// higher level resources:
mod static_website;
pub use static_website::*;

pub static mut BUILD_BUCKET: String = String::new();
pub static mut DEPLOY_REGION: Result<String, &'static str> = Err("us-east-1");
pub static mut STACK_NAME: String = String::new();
pub static mut BUILD_COMMANDS: Vec<String> = vec![];
pub static mut PACKAGE_COMMANDS: Vec<String> = vec![];
pub static mut DEPLOY_COMMANDS: Vec<String> = vec![];
pub static mut POST_COMMANDS: Vec<String> = vec![];
pub static mut RESOURCES: Vec<String> = vec![];
pub static mut PARAMETER_VALUES: Vec<(String, String)> = vec![];

pub fn get_deploy_region() -> String {
    unsafe {
        match &DEPLOY_REGION {
            Ok(s) => s.clone(),
            Err(e) => (*e).into(),
        }
    }
}

pub fn set_deploy_region<S: AsRef<str>>(region: S) {
    unsafe {
        DEPLOY_REGION = Ok(region.as_ref().into());
    }
}

pub fn add_build_cmd<S: AsRef<str>>(cmd: S) {
    unsafe {
        BUILD_COMMANDS.push(cmd.as_ref().into());
    }
}
#[allow(dead_code)]
pub fn add_package_cmd<S: AsRef<str>>(cmd: S) {
    unsafe {
        PACKAGE_COMMANDS.push(cmd.as_ref().into());
    }
}
#[allow(dead_code)]
pub fn add_deploy_cmd<S: AsRef<str>>(cmd: S) {
    unsafe {
        DEPLOY_COMMANDS.push(cmd.as_ref().into());
    }
}
pub fn add_post_cmd<S: AsRef<str>>(cmd: S) {
    unsafe {
        POST_COMMANDS.push(cmd.as_ref().into());
    }
}
pub fn add_param_value<S: AsRef<str>, S1: AsRef<str>>(p: (S, S1)) {
    unsafe {
        PARAMETER_VALUES.push((p.0.as_ref().into(), p.1.as_ref().into()));
    }
}