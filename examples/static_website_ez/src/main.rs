use hira::{
    close,
    set_deploy_region,
    set_stack_name,
    const_from_dot_env_or_default
};

// to deploy this you'll either need to create a .env file with ACM_ARN=aws:arn...
// or modify the default values in this file to be your cert ARN
const_from_dot_env_or_default!(ACM_ARN, "aws:arn:...your-acm-arn-here");
set_deploy_region!("us-east-1");
set_stack_name!("example-static-website-ez");


#[hira::module("hira:aws_s3", {
    bucket_name: "hira-example-static-website-ez",
    public_website: {},
})]
mod mybucket {
    // This gets called during deploy time to initialize your S3 bucket!
    pub async fn _init() {
        let client = self::make_s3_client().await;
        let html = "<html><body><h1>hello world</h1></body></html>";
        self::put_object_builder(&client, "index.html", html.into())
            .content_type("text/html")
            .send().await.expect("Failed to initialize S3 bucket");
    }
}

#[hira::module("hira:aws_r53_cdn", {
    acm_cert_arn: ACM_ARN,
})]
const MYSITE: () = match ("yourwebsite.com", "path") {
    ("yourwebsite.com", "*") => (mybucket::WEBSITE_URL, "*", "originProtocol=http-only"),
};

close!();
