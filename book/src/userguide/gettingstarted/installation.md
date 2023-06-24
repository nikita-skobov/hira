# Installation

> Note: This guide assumes you have the Rust toolchain installed.

One extra step you'll need to take after you install Rust, but before you can use Hira is to add the `wasm32-unknown-unknown` target via `rustup`. Hira needs to compile certain parts of your code into Webassembly during compile time, and as such it needs that target to be available:

```sh
rustup target add wasm32-unknown-unknown
```

Now you're ready to start using Hira. Note that Hira at its core is simply a Rust procedural macro, so you can simply add Hira to your existing Rust project by editing the Cargo.toml file and adding Hira to the `dependencies`.

> If you don't have an existing Rust project, open your terminal and enter:
> ```sh
> cargo new --lib myproj
> cd myproj
> ```


Open your Cargo.toml file and ensure your `dependencies` section has:

```toml
[dependencies]
hira = { git = "https://github.com/nikita-skobov/hira" }
```

## Hira CLI

Next, you will want to download the Hira CLI. The purpose of the Hira CLI is to simplify the build process. Hira CLI is simply a wrapper over `cargo build` + finalizing actions.

> The rest of this guide assumes you have the Hira CLI installed. Note that the Hira CLI is not strictly necessary, and you can use Hira simply by running `cargo build`. However, that is not the recommended way to use Hira. It is a lot more convenient to use the Hira CLI as it means your build becomes 1 step rather than 3.

There are a few ways to install the Hira CLI:

1. Compile from source:
    ```sh
    git clone https://github.com/nikita-skobov/hira
    cd hira
    cargo build --release -p hira_cli
    sudo cp target/release/hira_cli /usr/local/bin/hira
    ```
2. Download pre-built binary:
    ```sh
    curl -iLO https://github.com/nikita-skobov/hira/releases/latest/download/hira.zip
    unzip hira.zip
    chmod +x hira_cli
    sudo mv hira_cli /usr/local/bin/hira
    ```
