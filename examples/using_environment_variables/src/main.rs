use hira::{close, set_build_bucket, set_deploy_region, set_stack_name, secret_from_dot_env, const_from_dot_env_or_default, load_dot_env};
use serde_json::Value;

// this is optional. by default we assume your environment
// variables are in ".env". If you need to specify a different path you
// can call load_dot_env!() before anything else
load_dot_env!("MY_ENV_FILE.txt");

// load a variable from a .env file, or use default if not found:
const_from_dot_env_or_default!(BUILD_BUCKET, "put-your-bucket-here");

// load a variable from a .env file, but it is only available at compile time.
secret_from_dot_env!(SECRET_STACK_NAME);

// can either provide a const, or a string literal:
set_build_bucket!(BUILD_BUCKET);
set_deploy_region!("us-east-1");
set_stack_name!(SECRET_STACK_NAME);


#[hira::module("hira:aws_lambda", {
    triggers: [{ "type": "function_url" }],
})]
async fn hello_world(_event: Value) -> Result<String> {
    // this is valid because BUILD_BUCKET is a publically accessible constant
    println!("{}", BUILD_BUCKET);
    // This is NOT valid because we declared SECRET_STACK_NAME a secret,
    // and therefore it does not exist at runtime:
    // println!("{}", SECRET_STACK_NAME);

    Ok(format!("hello world"))
}

close!();
