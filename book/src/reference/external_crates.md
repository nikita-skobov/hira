# Using External Crates

Hira allows you to use any crate you want within your modules. The only caveats are:
1. They must be declared with `extern crate`. Hira needs to know the difference between using a Hira module, and using an external crate, and thus those imports have to be defined differently.
2. They must compile to `wasm32-unknown-unknown` target. Hira compiles Hira modules to wasm during compile time, so you must ensure that whatever crate you are using is valid in wasm32. Often times crates will have an extra feature to allow them to work in different contexts such as `no_std`, `wasm`, etc. You can add this feature to your dependency in your Cargo.toml file, and Hira will automatically pick up that the crate needs to be compiled with a specific feature to work. See [Cargo's documentation on specifying features here](https://doc.rust-lang.org/cargo/reference/features.html#dependency-features).

