This is the core functionality of hira.

The hira crate is just a proc-macro wrapper above this crate.

## Testing

Because the `src/lib.rs` file contains e2e tests that actually compile binaries, it's recommended to use 1 thread in the test program to ensure that each test case doesn't compete w/ each other for compiling the same wasm files. Please use:

```
cargo test -p hira_lib -- --test-threads=1
```
