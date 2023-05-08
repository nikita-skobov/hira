use std::collections::HashSet;

use proc_macro2::TokenStream;
use wasm_type_gen::*;

use crate::{HiraConfig, module_loading::{HiraModule2, OutputType}, parsing::compiler_error, wasm_types::to_map_entry};


#[derive(WasmTypeGen, Debug, Default)]
pub struct LibraryObj {
    // pub compiler_error_message: String,
    // pub add_code_after: Vec<String>,
    // /// crate_name is read only. modifying this has no effect.
    // pub crate_name: String,
    // pub user_data: UserData,
    // pub shared_output_data: Vec<SharedOutputEntry>,
    // /// a shared key/value store for accessing data across wasm module invocations.
    // /// this can be both used by you as a module writer, as well as optionally exposing
    // /// this to users by providing this data in your exported struct.
    // /// NOTE: this is append only + read. a wasm module cannot modify/delete existing key/value pairs
    // /// but it CAN read them, as well as append new ones.
    // pub shared_state: std::collections::HashMap<String, String>,
    // /// names of dependencies that the user has specified in their Cargo.toml.
    // /// NOTE: these are read only.
    // pub dependencies: Vec<String>,

    // everything below is a level0 capability for modulesV2:
    // NOTE: these MUST be named in the same way
    // as the struct, but in snake_case. The wasm code generator
    // will only see the type name "L0KvReader" and will
    // convert it to snake case
    pub l0_kv_reader: L0KvReader,

    /// Core L0 functionality.
    /// None of the functionality within core is marked as a capability
    /// because these are all library-approved actions, and thus shouldnt
    /// require user review. These are operations such as:
    /// - outputting compiler error messages
    /// - saving module outputs to be used by other functions
    pub l0_core: L0Core,

    pub l0_append_file: L0AppendFile,
}


impl LibraryObj {
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        self.l0_core.apply_changes(conf, module)?;
        self.l0_append_file.apply_changes(conf, module)?;
        Ok(())
    }
    pub fn initialize_capabilities(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        // core doesnt need any initialization (for now)
        self.l0_append_file.initialize_capabilities(conf, module)?;
        Ok(())
    }
}


#[derive(WasmTypeGen, Debug)]
pub struct SharedOutputEntry {
    pub filename: String,
    pub label: String,
    pub line: String,
    pub unique: bool,
    pub after: Option<String>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0KvReader {
    data: std::collections::HashMap<String, String>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0AppendFile {
    shared_output_data: Vec<SharedOutputEntry>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0Core {
    compiler_error_message: String,
    module_outputs: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    current_module_name: String,
}


impl L0AppendFile {
    pub fn initialize_capabilities(&mut self, _conf: &mut HiraConfig, _module: &mut HiraModule2) -> Result<(), TokenStream> {
        // TODO: actually i dont think this is necessary.
        // for file operations i think itll be easier to be optimistic, and just let the writers
        // put data in this struct, and then we verify that its valid when we leave the wasm context.
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        let mut all_transient_deps = HashSet::new();
        module.visit_lvl3_dependency_names(&conf, &mut |dep| {
            all_transient_deps.insert(dep.to_string());
        });
        // collect all the files these modules are allowed to access:
        let mut all_allowed_files = HashSet::new();
        for dep in all_transient_deps.iter() {
            if let Some(dep_module) = conf.get_mod2(dep) {
                for file in dep_module.file_capabilities.iter() {
                    all_allowed_files.insert(file);
                }
            }
        }
        // verify that all files that were provided were ones that this module was allowed to touch
        // TODO: technically this is wrong...
        // what this checks for is if ANY transient dependency specified this file
        // what we really want is to only allow specific modules to write to specific files.
        let mut out = Ok(());
        let contents: Vec<SharedOutputEntry> = self.shared_output_data.drain(..).map(|x| {
            if !all_allowed_files.contains(&x.filename) {
                out = Err(compiler_error(&format!("Module '{}' had a dependency that attempted to write file {}, but allowed files are only {:?}", module.name, x.filename, all_allowed_files)));
            }
            x
        }).collect();
        if out.is_err() {
            return out;
        }

        let mapped_data = to_map_entry(contents);
        conf.output_shared_files(&module.name, mapped_data)?;
        out
    }
}

impl L0Core {
    pub fn drain_outputs_into(&mut self, mod_name: &str, existing: &mut std::collections::HashMap<String, String>) {
        if let Some(mut kv_pairs) = self.module_outputs.remove(mod_name) {
            for (key, val) in kv_pairs.drain() {
                existing.insert(key, val);
            }
        }
    }
    pub fn remove_specific_output(&mut self, mod_name: &str, key: &str) -> Option<String> {
        let kv_pairs = self.module_outputs.get_mut(mod_name)?;
        Some(kv_pairs.remove(key)?)
    }
    pub fn set_defaults_recursively(&mut self, conf: &HiraConfig, dep_name: &str) {
        if let Some(module) = conf.get_mod2(dep_name) {
            if !self.module_outputs.contains_key(dep_name) {
                self.module_outputs.insert(dep_name.to_string(), Default::default());
            }
            let mut insert = vec![];
            for output in module.outputs.iter() {
                match output {
                    OutputType::AllFromModule(mod_name) => {
                        self.set_defaults_recursively(conf, mod_name);
                        if let Some(mod_outputs) = self.module_outputs.get(mod_name) {
                            for (key, val) in mod_outputs.iter() {
                                insert.push((key.to_string(), val.to_string()));
                            }
                        }
                    }
                    OutputType::SpecificFromModule(mod_name, key) => {
                        self.set_defaults_recursively(conf, mod_name);
                        if let Some(mod_outputs) = self.module_outputs.get(mod_name) {
                            if let Some(val) = mod_outputs.get(key) {
                                insert.push((key.to_string(), val.to_string()));
                            }
                        }
                    }
                    OutputType::SpecificConst(k, v) => {
                        insert.push((k.to_string(), v.to_string()));
                    }
                }
            }
            if let Some(kv_pairs) = self.module_outputs.get_mut(dep_name) {
                for (key, val) in insert {
                    if !kv_pairs.contains_key(&key) {
                        kv_pairs.insert(key, val);
                    }
                }
            }
        }
    }
    pub fn verify_outputs_and_set_defaults(&mut self, conf: &mut HiraConfig, first_lvl2_dep: &str) -> Result<(), TokenStream> {
        // visit all the transient dependencies, and insert their
        // default const outputs if not already overridden dynamically
        self.set_defaults_recursively(conf, first_lvl2_dep);
        for (mod_name, kv_pairs) in self.module_outputs.iter_mut() {
            let module = match conf.get_mod2(mod_name) {
                Some(m) => m,
                None => {
                    return Err(compiler_error(&format!("Detected outputs from intermediate module {} but this module doesn't exist", mod_name)));
                }
            };
            for (key, val) in kv_pairs.iter_mut() {
                if !module.has_output(key, conf) {
                    return Err(compiler_error(&format!("Detected output '{}' from intermediate module {} but this module did not specify such an output", key, mod_name)));
                }
            }
        }
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        let lvl2_dep_name = module.level3_get_depends_on(module.lvl3_module_depends_on.as_ref())?;
        self.verify_outputs_and_set_defaults(conf, &lvl2_dep_name)?;
        for output in module.outputs.iter() {
            match output {
                crate::module_loading::OutputType::AllFromModule(other_module_name) => {
                    self.drain_outputs_into(&other_module_name, &mut module.resolved_outputs);
                    break;
                }
                crate::module_loading::OutputType::SpecificFromModule(other_module_name, key) => {
                    if let Some(val) = self.remove_specific_output(other_module_name, key) {
                        module.resolved_outputs.insert(key.to_string(), val);
                    }
                }
                // shouldnt be possible to set this because
                // L3 modules cant use specific const outputs
                // and only L3 modules get apply_changes called on it
                crate::module_loading::OutputType::SpecificConst(_, _) => unreachable!(),
            }
        }
        Ok(())
    }
}


#[output_and_stringify_basic_const(FILE_IMPL)]
impl L0AppendFile {
    pub fn new() -> Self {
        Self { shared_output_data: Default::default() }
    }

    /// given a file name (no paths. the file will appear in ./wasmgen/{filename})
    /// and a label, and a line (string) append to the file. create the file if it doesnt exist.
    /// the label is used to sort lines between your wasm module and other invocations.
    /// the label is also embedded to the file. so if you are outputing to a .sh file, for example,
    /// your label should start with '#'. The labels are sorted alphabetically.
    /// Example:
    /// ```rust,ignore
    /// # wasm module 1 does:
    /// append_to_file("hello.txt", "b", "line1");
    /// # wasm module 2 does:
    /// append_to_file("hello.txt", "b", "line2");
    /// # wasm module 3 does:
    /// append_to_file("hello.txt", "a", "line3");
    /// # wasm moudle 4 does:
    /// append_to_file("hello.txt", "a", "line4");
    /// 
    /// # the output:
    /// a
    /// line3
    /// line4
    /// b
    /// line1
    /// line2
    /// ```
    #[allow(dead_code)]
    pub fn append_to_file(&mut self, name: &str, label: &str, line: String) {
        self.shared_output_data.push(SharedOutputEntry { label: label.into(), line, filename: name.into(), unique: false, after: None });
    }

    /// same as append_to_file, but the line will be unique within the label
    #[allow(dead_code)]
    pub fn append_to_file_unique(&mut self, name: &str, label: &str, line: String) {
        self.shared_output_data.push(SharedOutputEntry { label: label.into(), line, filename: name.into(), unique: true, after: None });
    }

    /// like append_to_file, but given a search string, find that search string in that label
    /// and then append the `after` portion immediately after the search string. Example:
    /// ```rust,ignore
    /// // "hello " doesnt exist yet, so the whole "hello , and also my friend Tim!" gets added
    /// append_to_line("hello.txt", "a", "hello ", ", and also my friend Tim!");
    /// append_to_line("hello.txt", "a", "hello ", "world"); 
    /// 
    /// # the output:
    /// hello world, and also my friend Tim!
    /// ```
    #[allow(dead_code)]
    pub fn append_to_line(&mut self, name: &str, label: &str, search_str: String, after: String) {
        self.shared_output_data.push(SharedOutputEntry { label: label.into(), line: search_str, filename: name.into(), unique: false, after: Some(after) });
    }
}


#[output_and_stringify_basic_const(KV_IMPL)]
impl L0KvReader {
    pub fn new() -> Self {
        Self { data: Default::default() }
    }

    // TODO: this capability is only supposed to be allowed to read...
    pub fn insert(&mut self, key: String, val: String) {
        self.data.insert(key, val);
    }
}


#[output_and_stringify_basic_const(CORE_IMPL)]
impl L0Core {
    pub fn new() -> Self {
        Self {
            compiler_error_message: Default::default(),
            module_outputs: Default::default(),
            current_module_name: Default::default(),
        }
    }
    // this is used by the code generator to ensure
    // that when each module's config function is called, this
    // sets the name such that if that module calls
    // "set_output", then it gets properly set into the module_outputs field
    #[doc(hidden)]
    pub fn set_current_module(&mut self, name: &str) {
        self.current_module_name = name.to_string();
    }
    /// set an output from your module. The key should correspond to
    /// the name of one of your outputs in your `mod outputs { }` section.
    /// case matters.
    pub fn set_output(&mut self, key: &str, val: &str) {
        match self.module_outputs.get_mut(&self.current_module_name) {
            Some(x) => {
                x.insert(key.to_string(), val.to_string());
            }
            None => {
                let mut map = std::collections::HashMap::new();
                map.insert(key.to_string(), val.to_string());
                self.module_outputs.insert(self.current_module_name.clone(), map);
            }
        }
    }
}


#[output_and_stringify_basic_const(LIBRARY_OBJ_IMPL)]
impl LibraryObj {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            l0_kv_reader: L0KvReader::new(),
            l0_core: L0Core::new(),
            l0_append_file: L0AppendFile::new(),
        }
    }
}

pub fn get_include_string() -> &'static [&'static str] {
    &[
        LIBRARY_OBJ_IMPL, FILE_IMPL, CORE_IMPL, KV_IMPL,
    ]
}
