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

fn main() {
    let file_path = std::env::args().nth(1).expect("Must provide path to file");
    let contents = std::fs::read_to_string(&file_path).expect(&format!("Failed to read {file_path}"));

    let curr_dir = std::env::current_dir().expect("Failed to get current dir");
    let curr_dir = curr_dir.to_string_lossy().to_string();
    // this depends on some env vars. this whole thing is a hack so im not gonna
    // set all of them, but just the ones i think matter
    std::env::set_var("CARGO_MANIFEST_DIR", curr_dir);
    let mut conf = hira_lib::HiraConfig::new();

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
        // the above simply parsed and saved the actual module we wish to compile.
        // now we create a fake lvl3 module that references that lvl2 module.
        // this will actually create a binary:
        let fake_lvl3_code = format!(r#"
        pub mod fake_{lvl2_module_name} {{
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
        // to save the name of fake_lvl2_module, so the next iteration doesnt wrongly
        // return that name instead of the actual lvl2_module we care about
        let _ = get_recently_loaded_module_name(&mut already_loaded, &mut conf);
    }).expect(&format!("Failed to parse {file_path} as rust code"));
}
