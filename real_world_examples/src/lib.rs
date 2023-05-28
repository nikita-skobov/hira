use hira::hira;
use aws_lambda::h_aws_lambda;

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
    pub fn config(_lambdainput: &mut h_aws_lambda::Input) {}

    pub fn lambda_main(a: h_aws_lambda::FunctionUrlEvent) -> String {
        format!("Other lambda received::\n{}", a.body)
    }
}
