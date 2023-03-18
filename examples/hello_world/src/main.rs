use hira::{close, set_build_bucket, set_deploy_region, set_stack_name};
use serde_json::Value;

hira::const_from_dot_env!(BUILD_BUCKET);
set_build_bucket!(BUILD_BUCKET);
set_deploy_region!("us-east-1");
set_stack_name!("example-hello-world");

#[hira::module("hira:aws_lambda", {
    triggers: [{ "type": "function_url" }],
    policy_statements: [{
        "action": "lambda:InvokeFunction",
        "resource": "arn:aws:lambda:*:*:function:apples"
    }],
})]
async fn hello_world(event: Value) -> Result<String> {
    println!("{:#?}", event);
    // this invokes the 'apples' lambda function
    let apples_str = apples(2usize).await?;
    Ok(format!("You have {apples_str}"))
}

#[hira::module("hira:aws_lambda", {})]
async fn apples(n: usize) -> Result<String> {
    Ok(format!("{n} apples"))
}

close!();
