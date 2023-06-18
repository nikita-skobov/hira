# Hira
Homoiconic Rust Actions

### What is 'Homoiconic'?

> From [wikipedia](https://en.wikipedia.org/wiki/Homoiconicity):
> > A language is homoiconic if a program written in it can be manipulated as data using the language




## What is Hira?

Hira is a rust procedural macro that can manipulate rust code at compile time to generate *anything*. This includes generating rust code within the crate being compiled, generating external crates and compiling them, generating cloud deployment infrastructure, code in other languages, etc.

While Hira was created with cloud infrastructure deployment in mind, at its core, Hira is just a framework for creating simple reusable and composable modules that offer [procedural macro](https://doc.rust-lang.org/reference/procedural-macros.html) capabilities in a safe manner.

## How does it work?

First, everything is a module. Module authors can write reusable modules like this:

```rs
#[hira]
pub mod deployer {
    use hira_lib::level0::L0Core;

    #[derive(Default)]
    pub struct Input {
        pub region: String,
        pub resource: String,
    }

    fn deploy_resource(s: &String) {
        // omitted for brevity
    }

    pub fn config(input: &mut Input, l0core: &mut L0Core) {
        if input.region != "us-east-1" {
            l0core.compiler_error(&format!("Invalid region {} resource can only be deployed in us-east-1", input.region));
            return;
        }
        deploy_resource(&input.resource);
    }
}
```

This example doesn't do much. The only interesting part here is in the config function. We dynamically check if the value is
valid, and if not, we emit a "compiler_error". We will see shortly how this works.

The above code example is a module that is written to be used by someone else. So an end-user can
then write their own module that uses the `deployer` module above. like this:

```rs
#[hira]
pub mod mylvl3mod1 {
    use super::deployer;
    pub fn config(input: &mut deployer::Input) {
        input.region = "us-west-2".to_string();
        input.resource = "something".to_string();
    }
}
```

Here's where the magic happens:

- **Hira compiles every rust module that has the `#[hira]` macro into a webassembly file**.
- **Hira dynamically creates an entrypoint that calls all of the config functions of all the modules in order**.
    - in the above example, hira would do the following:
    ```rs
    fn wasm_entrypoint(...) {
        let mut input = deployer::Input::default();
        mylvlv3mod1::config(&mut input);
        deployer::config(&mut input, &mut l0core); // <- all level 0 capabilities are passed into the entrypoint
    }
    ```
- **Hira then immediately executes this webassembly file**
    - The webassembly is sandboxed, and only has limited functionality that is provided by Hira.
    - In the `deployer` module example, we can see what functionality it is using by looking at the
      function parameters of the `config` function. One of our parameters is `l0core: &mut L0Core`. This
      contains core capabilities of hira such as writing compiler warnings/errors, and emitting outputs to be
      used by other modules. In our example all we do is emit a compiler error if the input is wrong.
- When the webassembly finishes executing, hira takes the output returned from the module(s) and applies
  those changes to the code. For example, in the case of a compiler error, hira's proc macro returns tokens
  that will cause a compiler error to the user with the user-friendly message that the module writer wrote.


This example shows a minimal example of how a module writer can expose complex proc-macro functionality to end users
in a simple interface.

However, hira is capable of so much more. For a full list see [capabilities](#capabilities)


## Capabilities

Hira is designed with capabilities in mind. A capability is some privileged action that the wasm code
is allowed to perform. Or rather: *hira* performs the action, and the wasm code is only allowed to specify declaratively what it wants hira to do.

A capability is therefore something that should be considered privileged. Hira provides a mechanism for module writers to statically define
the capabilities that their module requires to work. A module writer who wishes to have access to write files can define this capability in their module like this:


```rs
#[hira]
pub mod mylevel2mod2 {
    use super::L0AppendFile;

    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("FILES", &["hello.txt"]),
    ];

    #[derive(Default)]
    pub struct Input {}

    pub fn config(input: &mut Input, filewriter: &mut L0AppendFile) {
        // ...
    }
}
```

Hira statically reads their `CAPABILITY_PARAMS` to determine which capabilities, and what parameters for that capability are requested.

If this code runs and the level2 module attempts to write to a file other than `hello.txt` hira will deny this and show an error to the user.

The end user can ultimately review/allow/block all the different capabilities that their code requests, and even specific values: such as in
this example where we can see which files the code wants to have write access to.

The following is a list of all core capabilities (development still in progress)

- Core: no capability requests are required. Core only allows emitting compiler errors/warnings, and emitting output values to be shared by other modules.
- AppendFile: allows module writers to append/create the files they specify via the `FILES` capability parameter
- CodeRead: allows module writers to read specific functions from within the module they were called. CodeRead requires the module writer to specify specifically which functions they want to be able to read. Hira only provides the function signatures of the requested function(s).
- CodeWrite: allows module writers to emit rust code into the user's code. CodeWrite can either emit functions *within* the user's module (ie: scopes to only that module), or emit a function *outside* the user's module (ie: make global functions). In either case, the module writer is required to specify the names and locations of the code they wish to emit.
- more to come...

