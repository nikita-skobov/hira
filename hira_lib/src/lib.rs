use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Mutex;
use parsing::compiler_error;
use proc_macro2::TokenStream;
use toml::Table;
use wasm_type_gen::{WasmIncludeString, WASM_PARSING_TRAIT_STR};
use wasm_types::MapEntry;
use level0::*;

pub mod parsing;
pub mod module_loading;
pub mod wasm_types;
pub mod level0;

pub const HIRA_DIR_NAME: &'static str = "hira";
pub const HIRA_WASM_DIR_NAME: &'static str = "wasm_out";
pub const HIRA_GEN_DIR_NAME: &'static str = "generated";
pub const HIRA_MODULES_DIR_NAME: &'static str = "modules";


#[derive(Default, Debug)]
pub struct HiraConfig {
    pub cargo_directory: String,
    pub hira_directory: String,
    pub modules_directory: String,
    pub wasm_directory: String,
    pub gen_directory: String,
    /// this directory is in the user's target/ folder.
    /// its purpose is to cache the module source code such that
    /// if the user loads a dependency from another crate, as long as that
    /// dependency had the hira macro, then its source code gets
    /// saved, and then we can fetch it from the cache directory
    pub module_cache_directory: String,

    pub should_do_file_ops: bool,
    pub known_cargo_dependencies: HashSet<String>,
    pub shared_data: HashMap<String, String>,
    pub shared_file_data: Vec<MapEntry<MapEntry<String>>>,
    /// a map of module name to a string containing callback code that should
    /// run prior to any invocation of this module.
    pub default_callbacks: HashMap<String, String>,

    pub hira_base_code: String,

    pub modules2: HashMap<String, module_loading::HiraModule2>,
}

impl HiraConfig {
    fn get_mod2(&self, name: &str) -> Option<&module_loading::HiraModule2> {
        self.modules2.get(name)
    }
    fn new() -> Self {
        let mut out = Self::default();
        out.set_directories();
        out.load_cargo_toml();
        out.set_should_do_file_ops();
        out.set_base_code();

        out
    }

    fn set_base_code(&mut self) {
        let mut hira_base = LibraryObj::include_in_rs_wasm();
        hira_base.push_str(WASM_PARSING_TRAIT_STR);
        for s in get_include_string() {
            hira_base.push_str(s);
        }
        self.hira_base_code = hira_base;
    }

    fn set_should_do_file_ops(&mut self) {
        // through manual testing i've found that running cargo build uses RUST_BACKTRACE full
        // whereas the cargo command used by IDEs sets this to short. basically: dont output command
        // files every keystroke.. instead we only wish to do this when the user actually builds.
        let mut should_do = false;
        if let Ok(env) = std::env::var("RUST_BACKTRACE") {
            if env == "full" {
                should_do = true;
            }
        }
        // check for optional env vars set by users:
        if let Ok(env) = std::env::var("CARGO_WASMTYPEGEN_FILEOPS") {
            if env == "false" || env == "0" {
                should_do = false;
            } else if env == "true" || env == "1" {
                should_do = true;
            }
        }
        self.should_do_file_ops = should_do;
    }

    fn merge_shared_files(
        &mut self,
        wasm_module_name: &str,
        data: Vec<MapEntry<MapEntry<(bool, String, Option<String>)>>>
    ) -> Result<(), TokenStream> {
        // merge the current data with the previous data
        for entry in data {
            let file_name = entry.key;
            // this is how we enforce that shared files only get output
            // to the shared directory. basically: it can only be a file name, not a path.
            if file_name.contains("/") || file_name.contains("\\") {
                return Err(compiler_error(&format!("Wasm module '{wasm_module_name}' attempted to output a shared file outside the shared file directory {:?}", file_name)));
            }
            for file_data in entry.lines {
                let label = file_data.key;

                let label_entry = if let Some(e) = self.shared_file_data.iter_mut().find(|x| x.key == file_name) {
                    if let Some(l) = e.lines.iter_mut().find(|x| x.key == label) {
                        l
                    } else {
                        let index = e.lines.len();
                        e.lines.push(MapEntry { key: label.clone(), lines: vec![] });
                        &mut e.lines[index]
                    }
                } else {
                    let index = self.shared_file_data.len();
                    self.shared_file_data.push(MapEntry { key: file_name.clone(), lines: vec![MapEntry { key: label.clone(), lines: vec![] }] });
                    &mut self.shared_file_data[index].lines[0]
                };

                for (unique, line, after) in file_data.lines {
                    if unique {
                        if !label_entry.lines.contains(&line) {
                            label_entry.lines.push(line);
                        }
                        continue;
                    }
                    // if after is provided, then treat 'line' as a search string, and
                    // try to insert the after portion immediately after the search string.
                    // if not found, output a newline concatenation of line+after
                    if let Some(after) = after {
                        let found_str = label_entry.lines.iter_mut()
                            .find_map(|l| l.find(&line).map(|index| (l, index + line.len())));
                        if let Some((found_str, index)) = found_str {
                            // found, now insert the after portion at the index
                            found_str.insert_str(index, &after);
                        } else {
                            // not found, just concatenate and output
                            label_entry.lines.push(format!("{line}{after}"));
                        }
                        continue;
                    }
                    // otherwise, its just a normal line entry
                    label_entry.lines.push(line);
                }
            }
        }
        Ok(())
    }

    fn iterate_map_entry(
        file_entry: &mut MapEntry<MapEntry<String>>,
        mut cb: impl FnMut(&str) -> Result<(), TokenStream>
    ) -> Result<(), TokenStream> {
        // sort the labels alphabetically
        file_entry.lines.sort_by(|a, b| a.key.cmp(&b.key));

        for label_entry in file_entry.lines.iter() {
            let label = &label_entry.key;
            cb(label)?;
            cb("\n")?;
            for line in label_entry.lines.iter() {
                cb(line)?;
                cb("\n")?;
            }
        }
        Ok(())
    }

    // this is a test utility to verify file operations happen correctly
    // without needing to write out to disk
    #[allow(dead_code)]
    #[cfg(debug_assertions)]
    fn get_shared_file_data(&mut self, name: &str) -> Option<String> {
        let entry = self.shared_file_data.iter_mut()
            .find(|x| x.key == name)?;
        let mut out = "".to_string();
        let _ = Self::iterate_map_entry(entry, |s| {
            out.push_str(s);
            Ok(())
        });
        Some(out)
    }

    fn output_shared_files(
        &mut self,
        wasm_module_name: &str,
        data: Vec<MapEntry<MapEntry<(bool, String, Option<String>)>>>
    ) -> Result<(), TokenStream> {
        // set the wasm_module's data into the global shared data object.
        self.merge_shared_files(wasm_module_name, data)?;
        // we only wish to actually write to disk if this is a real build
        // (or if user explicitly enabled it via CARGO_WASMTYPEGEN_FILEOPS=1)
        if !self.should_do_file_ops {
            return Ok(());
        }

        // create dir if it doesnt exist yet
        let shared_dir = &self.gen_directory;
        let _ = std::fs::create_dir(&shared_dir);
        // iterate the shared data object and output to the shared file(s)
        for file_entry in self.shared_file_data.iter_mut() {
            let file_name = &file_entry.key;
            let file_path = format!("{shared_dir}/{file_name}");
            let mut out_f = std::fs::File::create(&file_path)
                .map_err(|e| compiler_error(&format!("Failed to create/open file while running module '{wasm_module_name}' {:?}\nError:\n{:?}", file_path, e)))?;

            Self::iterate_map_entry(file_entry, |s| {
                out_f.write_all(s.as_bytes())
                    .map_err(|e| compiler_error(&format!("Failed to write to file while running module '{wasm_module_name}' {:?}\nError:\n{:?}", file_path, e)))
            })?;
        }

        Ok(())
    }

    fn set_directories(&mut self) {
        let base_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(".".into());
        let target_dir = std::env::var("CARGO_HOME").unwrap_or(".".into());
        self.cargo_directory = base_dir;
        self.hira_directory = format!("{}/{HIRA_DIR_NAME}", self.cargo_directory);
        self.modules_directory = format!("{}/{HIRA_MODULES_DIR_NAME}", self.hira_directory);
        self.wasm_directory = format!("{}/{HIRA_WASM_DIR_NAME}", self.hira_directory);
        self.gen_directory = format!("{}/{HIRA_GEN_DIR_NAME}", self.hira_directory);
        self.module_cache_directory = format!("{}/{HIRA_DIR_NAME}/cached_modules", target_dir);
    }

    fn load_cargo_toml(&mut self) {
        let file_path = format!("{}/Cargo.toml", self.cargo_directory);
        let cargo_file_str = if let Ok(file_str) = std::fs::read_to_string(file_path) {
            file_str
        } else {
            return
        };
        let value = cargo_file_str.parse::<Table>().unwrap();
        let mut dependencies = HashSet::new();        
        if let Some(deps) = value.get("dependencies") {
            if let toml::Value::Table(deps) = deps {
                for (key, _) in deps {
                    dependencies.insert(key.clone());
                }
            }
        }
        self.known_cargo_dependencies = dependencies;
    }
}

static mut PERSISTED_DATA: Mutex<Option<HiraConfig>> = Mutex::new(None);

pub fn use_hira_config(mut cb: impl FnMut(&mut HiraConfig)) {
    unsafe {
        if let Ok(mut lock) = PERSISTED_DATA.lock() {
            if lock.is_none() {
                *lock = Some(HiraConfig::new());
            }
            if let Some(config) = lock.as_mut() {
                cb(config);
            }
        }
    }
}

#[cfg(test)]
pub mod e2e_tests {
    use std::str::FromStr;
    use proc_macro2::TokenStream;
    use crate::module_loading::{hira_mod2_inner};
    use super::*;

    pub fn assert_contains_str<Q: AsRef<str>, S: AsRef<str>>(search: Q, contains: S) {
        let search = search.as_ref();
        let contains = contains.as_ref();
        let contains_true = search.contains(contains);
        if !contains_true {
            assert_eq!(format!("Expected to find '{}'", contains), search);
        }
        // :shrug: why not
        assert!(contains_true);
    }

    fn e2e_module2_run(
        module_code: &[&str],
        conf_cb: impl Fn(&mut HiraConfig),
    ) -> Result<HiraConfig, TokenStream> {
        let res = e2e_module2_run_with_token_stream(module_code, conf_cb)?;
        Ok(res.0)
    }

    fn e2e_module2_run_with_token_stream(
        module_code: &[&str],
        conf_cb: impl Fn(&mut HiraConfig),
    ) -> Result<(HiraConfig, TokenStream), TokenStream> {
        let mut conf = HiraConfig::default();
        conf.set_base_code();
        let path = std::path::PathBuf::from("./test_out");
        let _ = std::fs::create_dir("test_out");
        let path = path.canonicalize().expect("Failed to canonicalize test_out directory");
        let full_path_str = path.to_string_lossy().to_string();
        conf.wasm_directory = full_path_str;

        conf_cb(&mut conf);
        let mut stream = TokenStream::new();
        for code in module_code {
            let code = TokenStream::from_str(code).expect("Failed to parse test case code");
            let out = hira_mod2_inner(&mut conf, code);
            match out {
                Ok(s) => {
                    stream = s;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok((conf, stream))
    }

    #[test]
    fn mod2_outputs_work() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub region: String,
                    }
                    pub mod outputs {
                        pub const REGION: &str = "";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                        l0core.set_output("REGION", input.region.as_str());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub mod outputs {
                        pub use lvl2mod::outputs::*;
                    }
                    pub fn config(input: &mut lvl2mod::Input) {
                        input.region = "us-east-2".to_string();
                    }
                }
            ),
        ];
        let conf = e2e_module2_run(&code, |_| {}).expect("Failed to compile");
        let module = conf.get_mod2("mylevel3mod").expect("Failed to find mylevel3mod");
        assert_eq!(module.resolved_outputs["REGION"], "us-east-2");
    }

    #[test]
    fn mod2_outputs_must_exist_if_outputted() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub region: String,
                    }
                    pub mod outputs {
                        pub const REGION: &str = "";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                        l0core.set_output("NOT_DEFINED", input.region.as_str());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub mod outputs {
                        pub use lvl2mod::outputs::*;
                    }
                    pub fn config(input: &mut lvl2mod::Input) {
                        input.region = "us-east-2".to_string();
                    }
                }
            ),
        ];
        let err = e2e_module2_run(&code, |_| {}).err().expect("Expected compilation to fail due to NOT_DEFINED");
        let err_str = err.to_string();
        assert_contains_str(err_str, "this module did not specify such an output");
    }

    #[test]
    fn mod2_outputs_get_defaulted_if_not_set() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub mod outputs {
                        pub const REGION: &str = "eu-west-1";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub mod outputs {
                        pub use lvl2mod::outputs::*;
                    }
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let conf = e2e_module2_run(&code, |_| {}).expect("Failed to compile");
        let module = conf.get_mod2("mylevel3mod").expect("Failed to find mylevel3mod");
        assert_eq!(module.resolved_outputs["REGION"], "eu-west-1");
    }

    #[test]
    fn mod2_can_output_shared_file_data() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0AppendFile;

                    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[("FILES", &["hello.txt"])];

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0AppendFile) {
                        l0core.append_to_file("hello.txt", "b", "line1".to_string());
                        l0core.append_to_file("hello.txt", "b", "line2".to_string());
                        l0core.append_to_file("hello.txt", "a", "line3".to_string());
                        l0core.append_to_file("hello.txt", "a", "line4".to_string());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let res = e2e_module2_run(&code,|_| {});
        let mut conf = res.ok().unwrap();
        let data = conf.get_shared_file_data("hello.txt").expect("Failed to find hello.txt");
        assert_eq!(data, "a\nline3\nline4\nb\nline1\nline2\n");
    }


    #[test]
    fn mod2_can_output_compiler_errors() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                        l0core.compiler_error("this is a custom error");
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let (_, stream) = e2e_module2_run_with_token_stream(&code, |_| {}).expect("Test case compilation failed");
        let stream_text = stream.to_string();
        assert_contains_str(stream_text, "this is a custom error");
    }

    #[test]
    fn mod2_can_output_compiler_warnings() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                        l0core.compiler_warning("this is a custom warning");
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let (_, stream) = e2e_module2_run_with_token_stream(&code, |_| {}).expect("Test case compilation failed");
        let stream_text = stream.to_string();
        assert_contains_str(stream_text, "this is a custom warning");
    }

    #[test]
    fn mod2_can_write_functions_outside_of_the_module() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0CodeWriter;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: u32,
                    }

                    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[("CODE_WRITE", &["fn_global:main"])];

                    pub fn config(input: &mut Input, l0writer: &mut L0CodeWriter) {
                        l0writer.write_global_fn("pub fn main()".to_string(), "".to_string());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let (_, stream) = e2e_module2_run_with_token_stream(&code, |_| {}).expect("Failed to compile");
        let stream_str = stream.to_string();
        assert_contains_str(stream_str, "pub fn main ()");
    }

    #[test]
    fn mod2_can_write_functions_inside_of_the_module() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0CodeWriter;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: u32,
                    }

                    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[("CODE_WRITE", &["fn_module:heyo"])];

                    pub fn config(input: &mut Input, l0writer: &mut L0CodeWriter) {
                        l0writer.write_internal_fn("pub fn heyo()".to_string(), "".to_string());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let (_, stream) = e2e_module2_run_with_token_stream(&code, |_| {}).expect("Failed to compile");
        let stream_str = stream.to_string();
        // we add an extra bracket to check if this is truly inside the module.
        // ie: if its the last function in the module then we expect to see something like:
        // pub mod ... {
        // ...
        // pub fn heyo() { }
        // }
        // ^ we are looking for this
        assert_contains_str(stream_str, "pub fn heyo () { } }");
    }

    #[test]
    fn mod2_can_provide_requested_fn_signatures() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0CodeReader;

                    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
                        ("CODE_READ", &["fn:hello_world"])
                    ];

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0: &mut L0CodeReader) {
                        // test will fail if this unwrap panics:
                        l0.get_fn("hello_world").unwrap();
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}

                    pub fn hello_world() {}
                }
            ),
        ];
        let res = e2e_module2_run(&code,|_| {});
        assert!(res.is_ok());
    }

    #[test]
    fn mod2_fn_signature_not_provided_if_not_requested() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0CodeReader;

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0: &mut L0CodeReader) {
                        let opt = l0.get_fn("hello_world");
                        if opt.is_some() {
                            panic!("test failed because i expected to not get hello_world");
                        }
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}

                    pub fn hello_world() {}
                }
            ),
        ];
        let res = e2e_module2_run(&code,|_| {});
        assert!(res.is_ok());
    }

    #[test]
    fn mod2_requires_file_permissions_to_be_defined_statically() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0AppendFile;

                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0AppendFile) {
                        l0core.append_to_file("you_got_hacked.txt", "b", "line1".to_string());
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let res = e2e_module2_run(&code,|_| {});
        let err = res.err().unwrap();
        let err_str = err.to_string();
        assert_contains_str(err_str, "had a dependency that attempted to write file you_got_hacked.txt, but allowed files are only");
    }

    #[test]
    fn mod2_can_depend_on_external_crates() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    extern crate serde_json;
                    use super::L0AppendFile;

                    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[("FILES", &["hello.txt"])];

                    #[derive(Default)]
                    pub struct Input {
                        pub unused: bool,
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0AppendFile) {
                        let val: serde_json::Value = serde_json::from_str("{\"hello\":\"world\"}").expect("Failed to deserialize");
                        let json_str = serde_json::to_string(&val).expect("Failed to serialize");
                        l0core.append_to_file("hello.txt", "b", json_str);
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let res = e2e_module2_run(&code,|_| {});
        let mut conf = res.unwrap();
        let data = conf.get_shared_file_data("hello.txt").expect("Failed to find hello.txt");
        assert_contains_str(data, r#"{"hello":"world"}"#);
    }


    #[test]
    fn mod2_outputs_not_set_if_explicit() {
        let code = [
            stringify!(
                pub mod lvl2mod {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub mod outputs {
                        pub const REGION: &str = "eu-west-1";
                        pub const OTHER_CONST: &str = "should not be set in mylevel3mod";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                    }
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod;
                    pub mod outputs {
                        pub use lvl2mod::outputs::REGION;
                    }
                    pub fn config(input: &mut lvl2mod::Input) {}
                }
            ),
        ];
        let conf = e2e_module2_run(&code, |_| {}).expect("Failed to compile");
        let module = conf.get_mod2("mylevel3mod").expect("Failed to find mylevel3mod");
        assert_eq!(module.resolved_outputs["REGION"], "eu-west-1");
        assert!(!module.resolved_outputs.contains_key("OTHER_CONST"));
    }

    #[test]
    fn mod2_lvl2_mods_can_wrap_other_lvl2_mods() {
        let code = [
            // first lvl2mod
            stringify!(
                pub mod lvl2mod_a {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub mod outputs {
                        pub const A1: &str = "lvlv2moda1";
                        pub const A2: &str = "lvlv2moda2";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {
                        l0core.set_output("A1", "hey!");
                    }
                }
            ),
            // 2nd lvl2mod
            stringify!(
                pub mod lvl2mod_b {
                    use super::L0Core;
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub mod outputs {
                        pub const B1: &str = "lvlv2modb1";
                        pub const B2: &str = "lvlv2modb2";
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core) {}
                }
            ),
            // the lvl2mod that the user uses:
            stringify!(
                pub mod lvl2mod_c {
                    use super::L0Core;
                    use super::{lvl2mod_a, lvl2mod_b};
                    #[derive(Default)]
                    pub struct Input {
                        pub _unused: bool,
                    }
                    pub mod outputs {
                        pub use lvl2mod_a::outputs::*;
                        pub use lvl2mod_b::outputs::*;
                    }
                    pub fn config(input: &mut Input, l0core: &mut L0Core, ainp: &mut lvl2mod_a::Input, binp: &mut lvl2mod_b::Input) {}
                }
            ),
            stringify!(
                pub mod mylevel3mod {
                    use super::lvl2mod_c;
                    pub mod outputs {
                        pub use lvl2mod_c::outputs::*;
                    }
                    pub fn config(input: &mut lvl2mod_c::Input) {}
                }
            ),
        ];
        let conf = e2e_module2_run(&code, |_| {}).expect("Failed to compile");
        let module = conf.get_mod2("mylevel3mod").expect("Failed to find mylevel3mod");
        assert_eq!(module.resolved_outputs["B1"], "lvlv2modb1");
        assert_eq!(module.resolved_outputs["B2"], "lvlv2modb2");
        assert_eq!(module.resolved_outputs["A1"], "hey!");
        assert_eq!(module.resolved_outputs["A2"], "lvlv2moda2");
    }
}
