use std::path::{PathBuf, Path};

use hira_lib::{parsing::iter_hira_modules, HiraConfig};
use proc_macro2::TokenStream;
use quote::ToTokens;

pub fn get_recently_loaded_module_name(already_loaded: &mut Vec<String>, conf: &mut HiraConfig) -> Option<String> {
    for (mod_name, _) in conf.modules2.iter() {
        if !already_loaded.contains(mod_name) {
            already_loaded.push(mod_name.to_string());
            return Some(mod_name.to_string());
        }
    }
    None
}

/// given a path to a .rs file, try to read it as a hira module,
/// and if successful, output its wasm, and module config file
/// to the output directory
pub fn output_wasm_and_lvl2_module_config(
    path: &PathBuf,
    out_dir: &PathBuf,
) {
    let contents = std::fs::read_to_string(&path).expect(&format!("Failed to read {:?}", path));
    let home = env!("CARGO_HOME");
    // this depends on some env vars. this whole thing is a hack so im not gonna
    // set all of them, but just the ones i think matter
    std::env::set_var("CARGO_MANIFEST_DIR", out_dir);
    std::env::set_var("CARGO_HOME", home);
    let mut conf = hira_lib::HiraConfig::new();
    let out_dir_str = out_dir.to_string_lossy().to_string();

    // this produces as small wasm files as possible. useful for web.
    let custom_codegen_opts = Some(vec![
        "-C", "debuginfo=0",
        "-C", "debug-assertions=off",
        "-C", "codegen-units=1",
        "-C", "embed-bitcode=yes",
        "-C", "strip=symbols",
        "-C", "opt-level=z",
        "--cfg", "feature=\"web\"",
    ]);
    // let custom_codegen_opts = None;
    let mut already_loaded = vec![];
    iter_hira_modules(&contents, &mut |mut item| {
        // remove attributes, since we dont want to try to compile #[hira]
        item.attrs.clear();
        let tokenstream = item.to_token_stream();
        hira_lib::module_loading::hira_mod2_inner_ex(&mut conf, tokenstream, true, true, custom_codegen_opts.clone()).expect("Failed to run hira_mod2_inner");
        let lvl2_module_name = match get_recently_loaded_module_name(&mut already_loaded, &mut conf) {
            None => panic!("encountered duplicate module"),
            Some(s) => s,
        };
        // output the module's definition
        let module_def = conf.get_mod2(&lvl2_module_name).expect("Failed to get module after parsing?...");
        module_def.cache_to_disk(&out_dir_str);
        // the above simply parsed and saved the actual module we wish to compile.
        // now we create a fake lvl3 module that references that lvl2 module.
        // this will actually create a binary:
        let fake_lvl3_code = format!(r#"
        pub mod web_{lvl2_module_name} {{
            /// underscore_to_dash
            extern crate sapp_jsutils;
            extern crate serde_json;
            extern crate serde;
            use super::{lvl2_module_name};
            pub fn config(_: &mut {lvl2_module_name}::Input) {{ }}
        }}
        "#);
        let tokenstream: TokenStream = fake_lvl3_code.parse().expect("failed to parse fake module as tokens");
        hira_lib::module_loading::hira_mod2_inner_ex(&mut conf, tokenstream, true, true, custom_codegen_opts.clone()).expect("Failed to run hira_mod2_inner on fake lvl3 module");
        let file_name = format!("web_{lvl2_module_name}.wasm");
        let output_location = format!("{}/{}", conf.wasm_directory, file_name);
        let dest = format!("{}/{}", out_dir_str, file_name);
        std::fs::copy(&output_location, &dest).expect(&format!("failed to copy {} to {}", output_location, dest));
        std::fs::remove_dir_all(&conf.hira_directory).expect(&format!("Failed to remove {}", conf.hira_directory));
        // to save the name of fake_lvl2_module, so the next iteration doesnt wrongly
        // return that name instead of the actual lvl2_module we care about
        let _ = get_recently_loaded_module_name(&mut already_loaded, &mut conf);
    }).expect(&format!("Failed to parse {:?} as rust code", path));
}

pub fn iterate_and_output_each(out_dir: &PathBuf, files: Vec<PathBuf>) {
    for file in files {
        let mut dir = file.clone();
        dir.pop();
        println!("CHANGING TO {:?}", dir);
        std::env::set_current_dir(&dir).expect(&format!("Failed to change dir to {:?}", dir));
        println!("RUNNING ON {:?}", file);
        output_wasm_and_lvl2_module_config(&file, out_dir);
        // std::fs::copy(from, to)
        println!("CHANGING BACK TO {:?}", out_dir);
        std::env::set_current_dir(&out_dir).expect(&format!("Failed to change dir to {:?}", out_dir));
    }
}

fn iter_dir<S: AsRef<Path>>(start_dir: S, cb: &mut impl FnMut(PathBuf)) {
    let readdir = std::fs::read_dir(start_dir.as_ref()).expect("Failed to read directory provided");
    for entry in readdir {
        let entry_ok = match entry {
            Ok(o) => o,
            Err(_) => continue
        };
        let ft = match entry_ok.file_type() {
            Ok(o) => o,
            Err(_) => continue,
        };
        if ft.is_dir() {
            iter_dir(entry_ok.path(), cb);
        } else {
            let file_name = entry_ok.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".rs") {
                cb(entry_ok.path());
            }
        }
    }
}

fn main() {
    let command = std::env::args().nth(1).expect("Must provide input");
    if command == "output_for_web" {
        let search_path = std::env::args().nth(2).expect("Must provide path to directory to iterate");
        let curr_dir = std::env::current_dir().expect("Failed to get current directory");
        let mut rust_files = vec![];
        iter_dir(search_path, &mut |filepath| {
            rust_files.push(filepath);
        });
        iterate_and_output_each(&curr_dir, rust_files);
        return;
    }
    let file_path = command;
    let path = PathBuf::from(file_path);
    let curr_dir = std::env::current_dir().expect("Failed to get current dir");
    output_wasm_and_lvl2_module_config(&path, &curr_dir);
}
