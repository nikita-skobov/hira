# hira
Homoiconic Rust Aws

### What is 'Homoiconic'?

> From [wikipedia](https://en.wikipedia.org/wiki/Homoiconicity):
> > A language is homoiconic if a program written in it can be manipulated as data using the language

## What is Hira?

Hira is a set of rust procedural macros that can manipulate rust code at compile time to create deployment infrastructure for AWS. A single rust binary project can be used to create an entire AWS application. This can be best explained via an example. Consider the following code:

```rs
use hira::{close, set_build_bucket, set_deploy_region, set_stack_name};
use serde_json::Value;

hira::const_from_dot_env!(BUILD_BUCKET);
set_build_bucket!(BUILD_BUCKET);
set_deploy_region!("us-east-1");
set_stack_name!("example-hello-world");

#[hira::module("hira:aws_lambda", {
    triggers: [{ "type": "function_url" }],
    policy_statements: [{
        "action": "lambda:InvokeFunction",
        "resource": "arn:aws:lambda:*:*:function:apples"
    }],
})]
async fn hello_world(event: Value) -> Result<String> {
    println!("{:#?}", event);
    // this invokes the 'apples' lambda function
    let apples_str = apples(2usize).await?;
    Ok(format!("You have {apples_str}"))
}

#[hira::module("hira:aws_lambda", {})]
async fn apples(n: usize) -> Result<String> {
    Ok(format!("{n} apples"))
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

- [X] Diff deployments: only deploy what has changed
- [ ] Build can be parallelized
- [X] cargo build can save cache between invocations
- [X] Modular extensability: allow users to create their own cloud macros easily
- [ ] JSON Templates instead of yaml
- [X] Example: Make an S3 website w/ custom domain
- [ ] Example: simple game that saves state in DynamoDB
- [ ] Static analysis to generate necessary permissions
- [ ] Custom "panic" handler plugin
- [ ] hira-server: setup server architecture using hira itself. hira-server uses fast parallelized EC2 containers and can cache previous builds for fast deploys.
- [ ] Example: lambda can create an ec2 by calling its function.
- [X] Environment variable + const tracking
- [X] Move all artifacts to use /hira folder
- [ ] Allow making a module that wraps another module, and your module gets counted as 1. useful because: then you can enforce each module gets separate sub-stack
- [X] Snapshot testing against example stacks.
- [ ] Store your state deployment model. you deploy state that you can compare against in custom functions.
