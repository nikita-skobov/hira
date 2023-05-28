use hira::hira;
use aws_lambda::h_aws_lambda;
use aws_cloudfront_distribution::aws_cloudfront_distribution;

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
}

/// by default other modules are grouped together in the same stack.
#[hira]
pub mod other_lambda_fn {
    use super::h_aws_lambda;

    pub mod outputs {
        pub use super::h_aws_lambda::outputs::LOGICAL_FUNCTION_URL_NAME;
    }

    pub fn config(_lambdainput: &mut h_aws_lambda::Input) {}

    pub fn lambda_main(a: h_aws_lambda::FunctionUrlEvent) -> String {
        format!("Other lambda received::\n{}", a.body)
    }
}

#[hira]
pub mod making_my_distr {
    extern crate cfn_resources;
    use super::other_lambda_fn::outputs::*;
    use super::aws_cloudfront_distribution;
    use self::aws_cloudfront_distribution::CustomOriginConfigOriginProtocolPolicyEnum;

    pub fn config(distrinput: &mut aws_cloudfront_distribution::Input) {
        let func_url = cfn_resources::get_att(LOGICAL_FUNCTION_URL_NAME, "FunctionUrl");
        let select_domain = cfn_resources::select_split(2, "/", func_url);
        distrinput.default_origin_domain_name = cfn_resources::StrVal::Val(select_domain);
        distrinput.default_origin_protocol_policy = CustomOriginConfigOriginProtocolPolicyEnum::Httpsonly;
    }
}
