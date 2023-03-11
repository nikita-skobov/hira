/// This example accomplishes the same as `static_website_ez` but all of
/// the infrastructure components are defined individually.
/// Additionally, we can customize the individual components.
/// For example, by using the `_init` hook, we can set an initial state for
/// the s3 bucket.

use hira::{
    close,
    set_deploy_region,
    set_stack_name,
    const_from_dot_env_or_default,
    create_s3,
    create_cloudfront_distribution,
    create_route53_record,
};

const_from_dot_env_or_default!(WEBSITE_URL, "your.website-here.com");
const_from_dot_env_or_default!(ACM_ARN, "aws:arn:...your-acm-arn-here");

set_deploy_region!("us-east-1");
set_stack_name!("example-static-website-custom");

#[create_s3({
    name: "mybuckethelloworld22321321322",
    public_website: {},
})]
pub mod my_website_bucket {
    // this is a special function used by hira.
    // if you provide a `_init` function in your S3 website configuration,
    // hira will invoke this function as a post-deploy hook.
    // the purpose of this is to let you define the initial state of your S3 bucket.
    pub async fn _init() {
        let client = make_s3_client().await;
        let html = r#"<!DOCTYPE html>
        <html>
            <head><title>My First Website!</title></head>
            <body>
                <h1>Hello World!</h1>
            </body>
        </html>"#;
        self::upload_file(&client, "index.html", html).await;
        let html = r#"<!DOCTYPE html>
        <html>
            <head><title>My First Error!</title></head>
            <body>
                <h1>UwU something went wrong</h1>
            </body>
        </html>"#;
        self::upload_file(&client, "error.html", html).await;
    }
    // note that you can also put whatever functions you want here!
    // this is not a special function, just a helper function that gets
    // called by _init()
    pub async fn upload_file(client: &aws_sdk_s3::Client, key: &str, data: &str) {
        self::put_object_builder(&client, key, data.into())
            .content_type("text/html")
            .send().await.expect(&format!("Failed to write {key}"));
    }
}


#[create_cloudfront_distribution({
    origins_and_behaviors: [{
        domain_name: "mybuckethelloworld22321321322.s3-website-us-east-1.amazonaws.com",
    }],
    name: "mybuckethelloworld22321321322",
    aliases: [WEBSITE_URL],
    acm_certificate_arn: ACM_ARN,
})]
pub mod my_cdn {}


#[create_route53_record({
    record_type: "A",
    name: WEBSITE_URL,
    alias_target_dns_name: "!GetAtt CDNmybuckethelloworld22321321322.DomainName",
    alias_target_hosted_zone_id: "Z2FDTNDATAQYW2", // this is static for all of AWS for aliases to CloudFront
    // see here: https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-properties-route53-aliastarget.html#cfn-route53-aliastarget-hostedzoneid
})]
pub mod my_record {}

close!();
