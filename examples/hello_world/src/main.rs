use hira::{close, create_lambda, set_build_bucket, set_deploy_region, set_stack_name};

set_build_bucket!("put-the-name-of-your-s3-bucket-here");
set_deploy_region!("us-east-1");
set_stack_name!("hello-world-stack4");

#[create_lambda]
async fn hello_world(_event: String) -> String {
    // this invokes the 'apples' lambda function
    let apples_str = apples(2).await;
    format!("You have {apples_str}")
}

#[create_lambda]
async fn apples(n: usize) -> String {
    format!("{n} apples")
}

close!();
