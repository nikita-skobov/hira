use hira::{close, create_lambda, set_build_bucket, set_deploy_region, set_stack_name, create_s3, const_from_dot_env_or_default};
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

#[create_s3({ name: "mybuckadsdsadsadsa321321" })]
pub mod mybucket {}

#[create_lambda({
    triggers: [{ "type": "function_url" }],
    policy_statements: [{
        "action": "s3:PutObject",
        "resource": "arn:aws:s3:::mybuckadsdsadsadsa321321/*"
    }],
})]
async fn mybucket_uploader(event: Value) -> Response {
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
    resp
}

close!();
