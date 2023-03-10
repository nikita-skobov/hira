use hira::{close, create_lambda, set_build_bucket, set_deploy_region, set_stack_name};
use serde_json::Value;

set_build_bucket!("put-the-name-of-your-s3-bucket-here");
set_deploy_region!("us-east-1");
set_stack_name!("hello-world-stack4");

#[create_lambda({
    triggers: [{ "type": "function_url" }],
    policy_statements: [{
        "action": "lambda:InvokeFunction",
        "resource": "arn:aws:lambda:*:*:function:apples"
    }],
})]
async fn hello_world(event: Value) -> String {
    println!("{:#?}", event);
    // this invokes the 'apples' lambda function
    let apples_str = apples(2).await;
    format!("You have {apples_str}")
}

#[create_lambda]
async fn apples(n: usize) -> String {
    format!("{n} apples")
}

close!();
