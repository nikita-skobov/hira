use hira::{
    close,
    set_deploy_region,
    set_stack_name,
    create_static_website,
    const_from_dot_env_or_default
};

// to deploy this you'll either need to create a .env file with these values
// or modify the default values in this file to be a domain you
// own and certificate for that domain.
const_from_dot_env_or_default!(WEBSITE_URL, "your.website-here.com");
const_from_dot_env_or_default!(ACM_ARN, "aws:arn:...your-acm-arn-here");

set_deploy_region!("us-east-1");
set_stack_name!("hello-world-stack5");

#[create_static_website({
    url: WEBSITE_URL,
    acm_arn: ACM_ARN,
})]
pub mod my_website {}

close!();
