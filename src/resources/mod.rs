pub use std::collections::HashMap;

pub use super::parsing::*;

mod lambda;
pub use lambda::*;

pub static mut BUILD_BUCKET: String = String::new();
pub static mut DEPLOY_REGION: String = String::new();
pub static mut STACK_NAME: String = String::new();
pub static mut BUILD_COMMANDS: Vec<String> = vec![];
pub static mut PACKAGE_COMMANDS: Vec<String> = vec![];
pub static mut DEPLOY_COMMANDS: Vec<String> = vec![];
pub static mut RESOURCES: Vec<String> = vec![];


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
