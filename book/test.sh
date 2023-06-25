#!/usr/bin/env bash
set -e

# to test properly we must point mdbook at an existing target/debug/deps directory
# in order to reference external crates (in our case hira, hira_lib).
# to do so, we will use a separate dir for this to avoid co-mingling with the target/ dir at the
# root of this project. the reason is that if we build different binaries within the hira/ project
# then they all get placed into target/debug/deps, and then theres different versions of the same
# packages which throws off rustdoc/mdbook.

cargo build -p h_echo --target-dir ./booktarget
mdbook test -L ./booktarget/debug/deps
rm -rf ./hira
rm build.sh
