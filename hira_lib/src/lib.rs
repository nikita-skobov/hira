use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Mutex;
use module_loading::HiraModule;
use toml::Table;
use wasm_type_gen::{WasmIncludeString, WASM_PARSING_TRAIT_STR};
use wasm_types::{MapEntry, LibraryObj, lib_obj_impl, user_data_impl, kv_obj_impl};

pub mod parsing;
pub mod module_loading;
pub mod wasm_types;

use crate::module_loading::load_module;
use crate::wasm_types::to_map_entry;

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

    pub should_do_file_ops: bool,
    pub known_cargo_dependencies: HashSet<String>,
    pub loaded_modules: HashMap<String, module_loading::HiraModule>,
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
        hira_base.push_str(lib_obj_impl());
        // hira_base.push_str(user_data_impl());
        hira_base.push_str(kv_obj_impl());
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
    ) -> Result<(), String> {
        // merge the current data with the previous data
        for entry in data {
            let file_name = entry.key;
            // this is how we enforce that shared files only get output
            // to the shared directory. basically: it can only be a file name, not a path.
            if file_name.contains("/") || file_name.contains("\\") {
                return Err(format!("Wasm module '{wasm_module_name}' attempted to output a shared file outside the shared file directory {:?}", file_name));
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
        mut cb: impl FnMut(&str) -> Result<(), String>
    ) -> Result<(), String> {
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
    ) -> Result<(), String> {
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
                .map_err(|e| format!("Failed to create/open file while running module '{wasm_module_name}' {:?}\nError:\n{:?}", file_path, e))?;

            Self::iterate_map_entry(file_entry, |s| {
                out_f.write_all(s.as_bytes())
                    .map_err(|e| format!("Failed to write to file while running module '{wasm_module_name}' {:?}\nError:\n{:?}", file_path, e))
            })?;
        }

        Ok(())
    }

    fn do_file_ops(
        &mut self,
        module_name: &str,
        lib_obj: &mut LibraryObj,
    ) -> Result<(), String> {
        // let map_entry_data = to_map_entry(std::mem::take(&mut lib_obj.shared_output_data));
        // self.output_shared_files(module_name, map_entry_data)
        Ok(())
    }

    fn save_shared_data(&mut self, data: HashMap<String, String>) {
        for (key, value) in data {
            if !self.shared_data.contains_key(&key) {
                self.shared_data.insert(key, value);
            }
        }
    }

    fn set_directories(&mut self) {
        let base_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(".".into());
        self.cargo_directory = base_dir;
        self.hira_directory = format!("{}/{HIRA_DIR_NAME}", self.cargo_directory);
        self.modules_directory = format!("{}/{HIRA_MODULES_DIR_NAME}", self.hira_directory);
        self.wasm_directory = format!("{}/{HIRA_WASM_DIR_NAME}", self.hira_directory);
        self.gen_directory = format!("{}/{HIRA_GEN_DIR_NAME}", self.hira_directory);
    }

    pub fn get_module(&mut self, module_name: &str) -> Result<&HiraModule, String> {
        let split_count = module_name.split("_").into_iter().count();
        if split_count != 2 {
            return Err(format!("{:?} is not a valid module name. Module names must have 1 underscore separating a namespace and a name", module_name));
        }
        if !self.loaded_modules.contains_key(module_name) {
            let x = load_module(self, module_name.to_string())?;
            self.loaded_modules.insert(x.name.clone(), x);
        }
        if let Some(m) = self.loaded_modules.get(module_name) {
            return Ok(m);
        }
        Err(format!("Failed to resolve module '{}' even after loading...", module_name))
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
mod e2e_tests {
    use std::str::FromStr;
    use proc_macro2::TokenStream;
    use quote::ToTokens;
    use syn::ItemFn;
    use crate::module_loading::{run_module_inner, load_module_from_file_string};
    use super::*;

    fn assert_contains_str<Q: AsRef<str>, S: AsRef<str>>(search: Q, contains: S) {
        let search = search.as_ref();
        let contains = contains.as_ref();
        let contains_true = search.contains(contains);
        if !contains_true {
            assert_eq!(format!("Expected to find '{}'", contains), search);
        }
        // :shrug: why not
        assert!(contains_true);
    }

    fn assert_doesnt_contain_str<Q: AsRef<str>, S: AsRef<str>>(search: Q, contains: S) {
        let search = search.as_ref();
        let contains = contains.as_ref();
        let contains_true = search.contains(contains);
        if contains_true {
            assert_eq!(format!("Didnt expected to find '{}'", contains), search);
        }
        // :shrug: why not
        assert!(!contains_true);
    }

    fn separate_item_and_attr_part(code: &str) -> (TokenStream, TokenStream) {
        let stream = TokenStream::from_str(code).expect("Failed to parse test case code as token stream");
        let mut item = syn::parse2::<ItemFn>(stream).expect("Failed to parse test case code");
        let mut attr_stream = TokenStream::new();
        for a in item.attrs.drain(..) {
            match a.meta {
                syn::Meta::Path(_) => todo!(),
                syn::Meta::List(a) => {
                    attr_stream.extend([a.tokens]);
                }
                syn::Meta::NameValue(_) => todo!(),
            }
        }
        let item_stream = item.to_token_stream();
        (attr_stream, item_stream)
    }

    fn e2e_module_run(
        user_code: &str,
        module_code: &str,
        conf_cb: impl Fn(&mut HiraConfig),
    ) -> Result<(HiraConfig, TokenStream), TokenStream> {
        let mut conf = HiraConfig::default();
        conf.set_base_code();
        let (attr, item) = separate_item_and_attr_part(user_code);
        conf.wasm_directory = "./test_out".to_string();
        let module = load_module_from_file_string(&mut conf, "a", module_code.to_string()).expect("test case provided invalid module code");
        conf.loaded_modules.insert(module.name.clone(), module);
        conf_cb(&mut conf);
        let out = run_module_inner(&mut conf, item, attr)?;
        Ok((conf, out))
    }

    fn e2e_module_run_reuse_config(
        conf: &mut HiraConfig,
        user_code: &str,
        module_code: &str,
    ) -> Result<TokenStream, TokenStream> {
        let (attr, item) = separate_item_and_attr_part(user_code);
        let module = load_module_from_file_string(conf, "a", module_code.to_string()).expect("test case provided invalid module code");
        conf.loaded_modules.insert(module.name.clone(), module);
        run_module_inner(conf, item, attr)
    }

    #[test]
    fn wasm_evaluation_works() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {
                    obj.a = 2;
                })]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    let mut something = Something { a: 1 };
                    cb(&mut something);
                    if something.a == 2 {
                        obj.compile_error("a is 2");
                    }
                }
            ),
            |_conf| {}
            );
            let (_, res) = res.ok().unwrap();
            let res_str = res.to_string();
            assert_contains_str(res_str, "a is 2");
    }

    #[test]
    fn wasm_modules_can_read_and_edit_user_input_names() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {})]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    let name = obj.user_data.get_name();
                    assert_eq!(name, "hello");
                    *name = "renamed_from_wasm".to_string();
                }
            ),
            |_conf| {}
        );
        let (_, res) = res.ok().unwrap();
        let res_str = res.to_string();
        assert_contains_str(&res_str, "renamed_from_wasm");
        assert_doesnt_contain_str(res_str, "hello");
    }

    #[test]
    fn wasm_modules_can_have_default_configs() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {
                    assert_eq!(obj.a, 100);
                })]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    let mut mydata = Something { a: 1 };
                    cb(&mut mydata);
                }
            ),
            |conf| {
                conf.default_callbacks.insert("my_mod".to_string(), r#"
                |obj: &mut my_mod::Something| {
                    obj.a = 100;
                }
                "#.to_string());
            }
        );
        let (_, _res) = res.ok().unwrap();
    }

    #[test]
    fn wasm_modules_have_access_to_known_cargo_dependencies() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {})]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    assert_eq!(obj.dependencies[0], "tokio");
                }
            ),
            |conf| {
                conf.known_cargo_dependencies.insert("tokio".to_string());
            }
        );
        let (_, _res) = res.ok().unwrap();
    }

    #[test]
    fn wasm_modules_can_output_shared_file_data() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {})]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    obj.append_to_file("hello.txt", "b", "line1".to_string());
                    obj.append_to_file("hello.txt", "b", "line2".to_string());
                    obj.append_to_file("hello.txt", "a", "line3".to_string());
                    obj.append_to_file("hello.txt", "a", "line4".to_string());
                }
            ),
            |_conf| {}
        );
        let (mut conf, _res) = res.ok().unwrap();
        let data = conf.get_shared_file_data("hello.txt").expect("Failed to find hello.txt");
        assert_eq!(data, "a\nline3\nline4\nb\nline1\nline2\n");
    }

    #[test]
    fn wasm_modules_can_store_and_read_shared_data() {
        let res = e2e_module_run(
            stringify!(
                #[hira(|obj: &mut my_mod::Something| {})]
                fn hello() {}
            ),
            stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    obj.shared_state.insert("hello".to_string(), "world".to_string());
                }
            ),
            |_conf| {}
        );
        let (mut conf, _) = res.ok().unwrap();
        
        let res = e2e_module_run_reuse_config(&mut conf,
            stringify!(
                #[hira(|obj: &mut my_mod2::Something| {})]
                fn hello2() {}
            ), stringify!(
                const HIRA_MODULE_NAME: &'static str = "my_mod2";
                type ExportType = Something;
                pub struct Something { pub a: u32 }
                pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {
                    assert_eq!(obj.shared_state["hello"], "world");
                }
            )
        );
        let _ = res.ok().unwrap();
    }
}
