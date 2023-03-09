# hira
Homoiconic Rust Aws

### What is 'Homoiconic'?

> From [wikipedia](https://en.wikipedia.org/wiki/Homoiconicity):
> > A language is homoiconic if a program written in it can be manipulated as data using the language

## What is Hira?

Hira is a set of rust procedural macros that can manipulate rust code at compile time to create deployment infrastructure for AWS. A single rust binary project can be used to create an entire AWS application. This can be best explained via an example. Consider the following code:

```rs
use hira::{close, create_lambda, set_build_bucket, set_deploy_region, set_stack_name};

set_build_bucket!("put-the-name-of-your-s3-bucket-here");
set_deploy_region!("us-east-1");
set_stack_name!("hello-world-stack4");

#[create_lambda({
    triggers: [{ "type": function_url }],
    policy_statements: [{
        "action": "lambda:InvokeFunction",
        "resource": "arn:aws:lambda:*:*:function:apples"
    }],
})]
async fn hello_world(event: Value) -> String {
    println!("{:#?}", event);
    // this invokes the 'apples' lambda function
    let apples_str = apples(2).await;
    format!("You have {apples_str}")
}

#[create_lambda]
async fn apples(n: usize) -> String {
    format!("{n} apples")
}

close!();


```

When you run `cargo build` this creates 2 artifacts in your current directory:
- deploy.sh
- deploy.yml

The yaml file is a cloudformation template which defines your resources, in this case two lambda functions: `hello_world` and `apples`. The deploy script will build all of your lambda functions individually, package them for lambda, and then deploy the cloudformation template.

When you deploy this cloudformation template, it will create 2 lambda functions. The `hello_world` function gets a FunctionUrl, and when you make an HTTP request to it, it will invoke the `apples` lambda function.

## How to use?

Take a look at the `examples/` directory for some examples. Each example can be built by `cd`ing into that example directory, and running `cargo build` and then `./deploy.sh`. For example to build the hello_world project you can:

```sh
cd examples/hello_world
cargo build
./deploy.sh
```

## Prerequisites

- access to an AWS account
- rust toolchain
- aws cli that has access to deploy cloudformation stacks, and write to S3
- `zip`
- [`cargo-cross`](https://github.com/cross-rs/cross)
    - cargo-cross depends on docker

## Roadmap

- [ ] Diff deployments: only deploy what has changed
- [ ] Modular extensability: allow users to create their own cloud macros easily
- [ ] JSON Templates instead of yaml
- [ ] Example: Make an S3 website w/ custom domain
- [ ] Example: simple game that saves state in DynamoDB
- [ ] Static analysis to generate necessary permissions
- [ ] Custom "panic" handler plugin

