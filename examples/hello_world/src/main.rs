use hira::{close, create_lambda, set_build_bucket, set_deploy_region, set_stack_name};

set_build_bucket!("put-the-name-of-your-s3-bucket-here");
set_deploy_region!("us-east-1");
set_stack_name!("hello-world-stack");

#[create_lambda]
fn hello_world(event: String) -> String {
    format!("Your event was {event}")
}

close!();
