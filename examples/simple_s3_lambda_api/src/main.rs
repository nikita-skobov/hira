use hira::{close, set_build_bucket, set_deploy_region, set_stack_name, const_from_dot_env_or_default};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::{time::{SystemTime, UNIX_EPOCH}, collections::HashMap};

const_from_dot_env_or_default!(BUILD_BUCKET, "put-your-bucket-here");

set_build_bucket!(BUILD_BUCKET);
set_deploy_region!("us-east-1");
set_stack_name!("example-simple-s3-lambda-api");

#[derive(Deserialize, Serialize)]
pub struct Response {
    #[serde(rename = "statusCode")]
    pub status_code: u32,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[hira::module("hira:aws_s3", { name: "hira-example-simple-s3-lambda-api-bucket" })]
pub mod mybucket {}

#[hira::module("hira:aws_lambda", {
    triggers: [{ "type": "function_url" }],
    policy_statements: [{
        "action": "s3:PutObject",
        "resource": "arn:aws:s3:::hira-example-simple-s3-lambda-api-bucket/*"
    }],
})]
async fn mybucket_uploader(event: Value) -> Result<Response> {
    let mut resp = Response {
        status_code: 202,
        headers: HashMap::new(),
        body: "ok!".into(),
    };
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH)
        .expect("Time went backwards").as_secs().to_string();
    if let Err(e) = mybucket::put_object(&since_the_epoch, event.to_string().into()).await {
        resp.status_code = 500;
        resp.body = format!("Error putting object: {e}");
    }
    Ok(resp)
}

close!();
