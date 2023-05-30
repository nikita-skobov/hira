use hira::hira;
use aws_lambda::h_aws_lambda;
use dotenv_reader::dotenv_reader;
use ::aws_cloudfront_distribution::lambda_url_distribution;

#[hira]
pub mod myvars {
    use super::dotenv_reader;

    pub mod outputs {
        pub const ACM_ARN: &str = "this will be replaced by the value in the .env file. If not found, this string will be used as the default";
        pub const MY_DOMAIN: &str = "something.com";
    }

    pub fn config(inp: &mut dotenv_reader::Input) {
        inp.dotenv_path = ".env";
    }
}

#[hira]
pub mod my_lambda {
    use super::h_aws_lambda;

    // the aws_lambda module
    // will generate a deployment script to compile
    // our code, and package it to be a lambda function
    // and then generates cloudformation template to deploy it.
    // here we can customize the deployment behavior, such as
    // changing the lambda function name, changing memory/timeout
    // custom roles, etc.
    pub fn config(lambdainput: &mut h_aws_lambda::Input) {
        lambdainput.extra_options.memory_size = 1024.into();
    }

    // when the aws_lambda module gets invoked, it looks for "lambda_main"
    // and parses our signature. It sees FunctionUrlEvent, which is a type
    // it defined, and is able to deduce that we want our lambda function to be triggered
    // via Lambda Function Url. It generates cloudformation code to ensure
    // our function can be invoked via a Function Url.
    pub fn lambda_main(a: h_aws_lambda::FunctionUrlEvent) -> String {
        format!("You sent me:\n{}", a.body)
    }

    pub mod outputs {
        pub use super::h_aws_lambda::outputs::*;
    }
}

/// by default other modules are grouped together in the same stack.
#[hira]
pub mod other_lambda_fn {
    use super::h_aws_lambda;

    pub mod outputs {
        pub use super::h_aws_lambda::outputs::*;
    }

    pub fn config(_lambdainput: &mut h_aws_lambda::Input) {}

    pub fn lambda_main(a: h_aws_lambda::FunctionUrlEvent) -> String {
        format!("Other lambda received::\n{}", a.body)
    }
}

#[hira]
pub mod making_my_distr {
    extern crate cfn_resources;

    use super::myvars::outputs::{ACM_ARN, MY_DOMAIN};
    use super::my_lambda::outputs::LOGICAL_FUNCTION_URL_NAME as FIRST_URL;
    use super::other_lambda_fn::outputs::LOGICAL_FUNCTION_URL_NAME as OTHER_URL;
    use super::lambda_url_distribution;
    use self::lambda_url_distribution::LambdaApiEndpoint;
    use self::lambda_url_distribution::CustomDomainSettings;

    pub fn config(distrinput: &mut lambda_url_distribution::Input) {
        distrinput.custom_domain_settings = Some(
            CustomDomainSettings {
                acm_arn: ACM_ARN.to_string(),
                domain_name: MY_DOMAIN.to_string(),
                subdomain: Some("hadsadsadsa".to_string()),
                ..Default::default()
            }
        );
        distrinput.endpoints = vec![
            LambdaApiEndpoint { path: "/".into(), function_url_id: FIRST_URL.into() },
            LambdaApiEndpoint { path: "/test".into(), function_url_id: OTHER_URL.into() },
        ]
    }
}
