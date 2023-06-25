# Hello Hira

We will start by making a basic hello world application. Ensure you are in a Rust project that was created with `cargo new --lib`. Your project structure should look like this:

```txt
src/
    lib.rs
Cargo.toml
```

Edit your Cargo.toml file and add in the following under `dependencies`:

```toml
hira = { git = "https://github.com/nikita-skobov/hira" }
h_echo = { git = "https://github.com/nikita-skobov/hira" }
# The hira repository is structured as a Cargo workspace which is
# why we can use the same git URL to reference different packages at that URL
```


Now, edit your `src/lib.rs` file and copy paste the following:

<!-- To test this properly, need to invoke with: mdbook test -L ../target/debug/deps -->
<!-- And you need to ensure target/debug/deps does not contain duplicate versions of a pacakge... -->

```rust
# extern crate h_echo;
# extern crate hira;
# fn main() {}
use hira::hira;
use h_echo::echo;

#[hira]
pub mod hello_world {
    use super::echo;
    pub fn config(input: &mut echo::Input) {
        input.echo = "Hello From Hira!".into();
    }
}
```

Now run the Hira CLI from your project's directory and specify the name of the runtime you want Hira to build. In this case we will specify `hello_world`:

```sh
# ensure you are running this from anywhere within your project's directory
hira hello_world
```

You should see output that looks something like this:

```txt
Scanning all rust files from "/home/projects/myproj"
Analyzing hello_world
Building runtime hello_world
    Finished dev [unoptimized + debuginfo] target(s) in 58.78s                  
Running hello_world:

Hello Hira!
```

Notice the compile time! It took my computer about a minute to build hira for the first time. This is typical for most rust projects: initial builds take a long time, but subsequent builds are significantly faster. Try editing your `hello_world` module to change the echo statement and recompile. For example try this:

```rust,ignore
input.echo = "Hello From Hiraaaaaaaa!!!!".into();
```

And now running `hira hello_world` again you should see output like this:

```txt
Scanning all rust files from "/home/projects/myproj"
Analyzing hello_world
Building runtime hello_world
    Finished dev [unoptimized + debuginfo] target(s) in 1.23s                   
Running hello_world:

Hello From Hiraaaaaaaa!!!!
```
