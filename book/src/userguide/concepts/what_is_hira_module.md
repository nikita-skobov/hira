# What is a Hira module?

A Hira module is a Rust module annotated with the `#[hira]` macro. Hira modules declare infrastructure, and Hira then creates the infrastructure as specified by the user.

In our hello world example we created a Hira module that looks like this:

```rust,ignore
#[hira]
pub mod hello_world {
    use super::echo;
    pub fn config(input: &mut echo::Input) {
        input.echo = "Hello From Hira!".into();
    }
}
```

All code within a Hira module will eventually be compiled to wasm, and that wasm code will be executed during compile time. Hira generates necessary code to execute all config functions for all modules that you specify. For example, let's take a quick peek under the hood to see what Hira will generate from our module:

```rust,ignore
let mut runtime: hira_lib::level0::L0RuntimeCreator = /* hira creates and instantiates this internally */;
let mut echoinput = echo::Input::default();
hello_world::config(&mut echoinput);
echo::config(&mut echoinput, &mut runtime);
runtime.apply_changes(/* hira internal code here */);
```

> Hira modules compile to wasm only during compile time. This does not mean the final runtimes are in wasm.

This allows users to define semantics of what their infrastructure needs at compile time. A Hira module is a declaration of desired infrastructure, and then lower level dependent modules (in our example the `echo` module) handle the creation of said infrastructure.

## Structure of a Hira Module

When Hira parses a module it looks for the following:

1. Must be a module and must be public. The `#[hira]` macro can only be applied to a module, ie code that starts with `pub mod hello_world { ... }`. Hira requires modules to be declared public via `pub`.
2. Use statements. Hira modules are only allowed to use very specific use statements:
    1. Other Hira modules.
        Example:
        ```rust,ignore
        use super::echo;
        ```
        Hira allows your module to "depend on" another module, and compose functionality by combining multiple modules together. In our `hello_world` module, we depend on another module called `echo`.
    2. Outputs.
        Example:
        ```rust,ignore
        use super::echo::outputs::XYZ;
        // OR:
        use super::echo::outputs::*;
        // OR:
        use super::echo::outputs::XYZ as XYZRenamed;
        ```
        Hira allows use statements that reference an output from another Hira module. In our case we did not use any outputs from the `echo` module because that module does not export any outputs, but we will [learn more about outputs in a later chapter](./outputs.md).
3. Config function.
    Example:
    ```rust,ignore
    pub fn config(input: &mut echo::Input) { ... }
    ```
    All Hira modules have a config function. It must be public, ie has `pub`, and must not return anything. All inputs must be mutable references, ie `&mut X`. The config function is compiled by Hira into webassembly and ran during compile time. **The important part about config functions is that Hira parses the signature and generates calling code with the desired inputs. You as a user declare what inputs you want to be given, and Hira provides them to you.**
4. Outputs.
    Example:
    ```rust,ignore
    #[hira]
    pub mod hello_world {
        pub mod outputs {
            pub const HELLO: &str = "WORLD";
        }
        // rest omitted for brevity
    }
    ```
    Our example did not specify any outputs, but we could have. Any Hira module can specify an `outputs` module within the Hira module. The `outputs` module can then be referenced by other modules by adding use statements that either import a specific named output, or all outputs. Outputs are useful for module interoperability; for example one module can create a website, and another module can reference the website URL as an output. In this example we declare a static output called `HELLO` with a value `WORLD`. Our Hira module can actually dynamically set the value of `HELLO` during compile time. And other modules that reference `hello_world` will get the dynamic value, rather than the default of `WORLD`. We'll [learn more about outputs in a later chapter](./outputs.md).
5. Inputs.
    Example:
    ```rust,ignore
    #[hira]
    pub mod echo {
        #[derive(Default)]
        pub struct Input {
            pub echo: String
        }
        // rest omitted for brevity
    }
    ```
    Some Hira modules can have an Input struct. Input structs must have a Default implementation, either via `#[derive(Default)]` or manually implementing it via `impl Default for Input { ... }`. Hira treats modules differently depending on if they have an Input struct. Our `hello_world` module does not have an Input struct, and that tells Hira that `hello_world` is a top-level module (we will learn more about [module types](./module_types.md) in a later chapter) and thus needs to run at compile time. On the other hand, the `echo` module *does* have an Input struct, and that tells Hira that the `echo` module is actually a component, and it is meant to be used by other modules, not ran on its own. If your Hira module contains an Input struct you must also include your own Input in your config function signature. This is referred to as a Self Input.
6. External Crates.
    Example:
    ```rust
    # extern crate h_echo;
    # extern crate hira;
    # fn main() {}
    use hira::hira;
    use h_echo::echo;

    #[hira]
    pub mod hello_world_with_serde {
        extern crate serde_json;
        use super::echo;
        pub fn config(input: &mut echo::Input) {
            let _something = serde_json::Value::String("hi".to_string());
            input.echo = "Hello From Hira!".into();
        }
    }
    ```
    Our example did not use any external crate, but in general, you can add any crate you want to your Hira module if you need extra functionality. Just keep in mind that any crate added must be compileable to a `wasm32-unknown-unknown` target. [See more information about using external crates in the reference.](../../reference/external_crates.md)
