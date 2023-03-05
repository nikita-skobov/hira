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
set_stack_name!("hello-world-stack");

#[create_lambda]
async fn hello_world(_event: String) -> String {
    // this calls lambda:InvokeFunction on the 'apples' lambda!
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

