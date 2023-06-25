# Hello Hira

Now we will look at a simple hello world program to define an application using Hira modules. We will learn more in later chapters what everything below does, and how it works, but in this chapter we will just focus on the code.

Assuming you have a Rust project with directory structure like:

```sh
src/
    lib.rs
Cargo.toml
```

Edit your Cargo.toml file and add in the following under `dependencies`:

```toml
hira = { git = "https://github.com/nikita-skobov/hira" }
hira_lib = { git = "https://github.com/nikita-skobov/hira" }
# The hira repository is structured as a Cargo workspace which is
# why we can use the same git URL to reference different packages
```

In the installation guide we only saw 1 dependency: `hira`. But now we are introducing `hira_lib`. `hira_lib` contains low level functionality that we will use for this hello world example.

> When using Hira you only need `hira_lib` if you are writing lower level modules that interface with Hira directly (which is what we will do in this tutorial).

Now, edit your `src/lib.rs` file and copy paste the following:

<!-- To test this properly, need to invoke with: mdbook test -L ../target/debug/deps -->
<!-- And you need to ensure target/debug/deps does not contain duplicate versions of a pacakge... -->

```rust
# extern crate hira_lib;
# extern crate hira;
# fn main() {}
use hira::hira;
use hira_lib::level0::L0Core;

#[hira]
pub mod reusable_module {
    use super::L0Core;
    #[derive(Default)]
    pub struct Input {
        /// We will impose an arbitrary restriction that
        /// valid values are only 1-10
        pub value: u32
    }
    pub fn config(self_input: &mut Input, l0core: &mut L0Core) {
        if self_input.value < 1 || self_input.value > 10 {
            l0core.compiler_error(&format!("Invalid value {}", self_input.value));
        }
    }
}
```

Congratulations! You have just written your first Hira module.

Let's go over the structure of this module, and show what this will actually do when compiled.

<!-- First we have our imports: `hira` and `hira_lib::level0::L0Core`. Hira is simply the crate that contains the macro, however `hira_lib` contains a lot of core functionality. In our case we are using `L0Core` which is a low-level capability. You can [learn more about capabilities](../concepts/capabilities.md) in a later chapter, but for now all you need to know is that `L0Core` is a struct that Hira provides, and it has some core low-level functionalities. -->

When we write `pub mod reusable_module` we declare a [Rust module](https://doc.rust-lang.org/stable/rust-by-example/mod/visibility.html), but what makes it special is we add the `#[hira]` macro above it. The Hira macro is a procedural macro, which means at compile tile the Rust compiler will actually call a function that Hira defines, and Rust will pass in everything below the `#[hira]` line to Hira. That is to say: **Hira will read your code within `reusable_module` and can parse, modify, and potentially even run parts of your code depending on what your module is designed to do.**

<!-- In this case, Hira won't compile/invoke our code during compile time. The reason is that we have included an `Input` struct. This tells Hira that this is a level 2 module. You can [learn more about module types here](../concepts/module_types.md), but for now all you need to know is that providing an `Input` struct tells Hira that this code shouldn't be executed during compile time. Instead, this tells Hira that this module is actually designed to be referenced later. Think of this as a component that can be re-used in your application many times, or even published for other users to include it as a component in their application. -->

However, in this case Hira won't run anything. That is because this is a module designed to be re-used, not invoked as is. Hira knows this because we included an `Input` struct which tells Hira that this module is meant to be used by other modules.

Let's now write another module that will use the above module. Copy and paste this code snippet below:

```rust
# extern crate hira_lib;
# extern crate hira;
# fn main() {}
# use hira::hira;
# use hira_lib::level0::L0Core;
# 
# #[hira]
# pub mod reusable_module {
#     use super::L0Core;
#     const X: usize = 1;
#     #[derive(Default)]
#     pub struct Input {
#         /// We will impose an arbitrary restriction that
#         /// valid values are only 1-10
#         pub value: u32
#     }
#     pub fn config(self_input: &mut Input, l0core: &mut L0Core) {
#         if self_input.value < 1 || self_input.value > 10 {
#             l0core.compiler_error(&format!("Invalid value {}", self_input.value));
#         }
#     }
# }

#[hira]
pub mod my_module {
    use super::reusable_module;
    # const Y: usize = 2;
    pub fn config(inp: &mut reusable_module::Input) {
        inp.value = 2;
    }
}
```

This `my_module` is also a Hira module because it has the `#[hira]` macro above it. However, this module is missing an `Input` struct. This tells Hira that this module is meant to be ran and so Hira will invoke it at compile time.

We're going to quickly peek under the cover of how Hira works. At a high level, Hira will do the following with your code:

```rust,ignore
let mut l0core = L0Core::default();
let mut reusable_input = reusable_module::Input::default();
my_module::config(&mut reusable_input);
reusable_module::config(&mut reusable_input, &mut l0core);
l0core.perform_actions();
```

Hira will generate code that looks something like the above, and then it will compile this code to a wasm file.

> Note, Hira generates a lot more code than this, but this is the core calling logic of how Hira calls our modules.

Then Hira will actually run the Wasm file during compilation. That's a Rust compilation within a Rust compilation! By compiling your code to wasm Hira enables end-users to insert customized compile-time behavior. You as the end user can now plugin directly into the compilation!

Now, remember that we had a `compiler_error()` call in our `reusable_module` earlier. Our `my_module` actually compiled fine, and the reason is that inside the `my_module::config` function, we set the value to 2. Which is within the range that we consider valid in `reusable_module`. Let's change our example code to actually set this value to be outside of the valid range and see what happens:

```rust,compile_fail
# extern crate hira_lib;
# extern crate hira;
# fn main() {let _ = 1; }
# use hira::hira;
# use hira_lib::level0::L0Core;
# 
# #[hira]
# pub mod reusable_module {
#     use super::L0Core;
#     const X: usize = 2;
#     #[derive(Default)]
#     pub struct Input {
#         pub value: u32
#     }
#     pub fn config(self_input: &mut Input, l0core: &mut L0Core) {
#         if self_input.value < 1 || self_input.value > 10 {
#             l0core.compiler_error(&format!("Invalid value {}", self_input.value));
#         }
#     }
# }

#[hira]
pub mod my_module2 {
    use super::reusable_module;
    # const Y: usize = 3;
    pub fn config(inp: &mut reusable_module::Input) {
        inp.value = 100;
    }
}
```

Now if you try to compile this you will get a compiler error that says:

```txt
error: Invalid value 100
```
