hira has many examples, and each example outputs several files including:
- a deploy.sh script
- a deploy.yml cloudformation template
- binaries relevant to the example

To make it easier to develop and check for breaking changes, I've added a simple snapshot-testing
framework that will:
- iterate over each example, and run `cargo build`
- save the deploy.yml and deploy.sh to a snapshot-testing directory

And after modifying an example, or changing the hira core library, it will run all of the examples again and compare the new artifacts against the ones saved in snapshot-testing.


# Using:

To use this snapshot-testing framework, first install the tool:
```sh
# assuming you are in the root of the hira/ directory
cd testing/
cargo build --release
cd ..
./test
```
