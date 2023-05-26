use std::{collections::{HashSet}, str::FromStr};

use proc_macro2::TokenStream;
use syn::{ItemMod, ItemFn, Item};
use quote::{ToTokens};
use wasm_type_gen::*;

use crate::{HiraConfig, module_loading::{HiraModule2, OutputType}, parsing::{compiler_error, iterate_mod_def_generic, parse_fn_signature}, wasm_types::{to_map_entry, FunctionSignature}};


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

    pub l0_code_reader: L0CodeReader,

    pub l0_code_writer: L0CodeWriter,

    pub l0_runtime_creator: L0RuntimeCreator,
}


impl LibraryObj {
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2, stream: &mut TokenStream) -> Result<(), TokenStream> {
        self.l0_core.apply_changes(conf, module, stream)?;
        self.l0_append_file.apply_changes(conf, module, stream)?;
        self.l0_code_writer.apply_changes(conf, module, stream)?;
        self.l0_runtime_creator.apply_changes(conf, module, stream)?;
        Ok(())
    }
    pub fn initialize_capabilities(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        self.l0_core.initialize_capabilities(conf, module)?;
        self.l0_append_file.initialize_capabilities(conf, module)?;
        self.l0_code_reader.initialize_capabilities(conf, module)?;
        self.l0_code_writer.initialize_capabilities(conf, module)?;
        self.l0_runtime_creator.initialize_capabilities(conf, module)?;
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
    current_module_name: String,
    data: std::collections::HashMap<String, String>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0AppendFile {
    shared_output_data: Vec<SharedOutputEntry>,
    current_module_name: String,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0Core {
    compiler_error_message: String,
    compiler_warning_message: String,
    module_outputs: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    current_module_name: String,
    lvl3_module_name: String,
    crate_name: String,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0CodeReader {
    current_module_name: String,
    function_signatures: std::collections::HashMap<String, FunctionSignature>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0CodeWriter {
    current_module_name: String,
    functions: std::collections::HashMap<String, std::collections::HashMap::<String, String>>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct L0RuntimeCreator {
    current_module_name: String,
    runtimes: std::collections::HashMap<String, RuntimeData>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct RuntimeInfo {
    pub creator: String,
    pub code: String,
    pub unique_line: bool,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct RuntimeData {
    pub code_lines: Vec<RuntimeInfo>,
    pub meta: RuntimeMeta,
}

#[derive(WasmTypeGen, Debug, Default, Clone)]
pub struct RuntimeMeta {
    pub cargo_cmd: String,
    pub target: String,
    pub profile: String,
}

#[derive(Default, Debug)]
struct FillCodeReader {
    function_signatures: std::collections::HashMap<String, FunctionSignature>,
    requested_fns: HashSet<String>,
}

fn set_functions(filler: &mut FillCodeReader, item: &mut ItemFn) {
    let name = item.sig.ident.to_string();
    if !filler.requested_fns.contains(&name) { return }

    let sig = parse_fn_signature(&item);
    filler.function_signatures.insert(name, sig);
}

fn get_all_capability_params(conf: &HiraConfig, module: &HiraModule2, capability_names: &[&str]) -> std::collections::HashMap<String, Vec<(String, String)>> {
    // find all transient modules that might have requested this capability
    let mut all_transient_deps = HashSet::new();
    module.visit_lvl3_dependency_names(&conf, &mut |dep| {
        all_transient_deps.insert(dep.to_string());
    });
    // find all the requested capabilities across all modules:
    let mut out = std::collections::HashMap::new();
    // ensure each capability name gets inserted as an empty vec
    for name in capability_names {
        out.insert(name.to_string(), vec![]);
    }
    for dep in all_transient_deps.iter() {
        if let Some(module) = conf.get_mod2(dep) {
            for (name, val) in out.iter_mut() {
                if let Some(params) = module.get_capability_params(name) {
                    for p in params {
                        val.push((dep.to_string(), p.to_string()));
                    }
                }
            }
        }
    }

    out
}

impl L0CodeWriter {
    pub fn initialize_capabilities(&mut self, _conf: &mut HiraConfig, _module: &mut HiraModule2) -> Result<(), TokenStream> {
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2, stream: &mut TokenStream) -> Result<(), TokenStream> {
        // skip expensive calculations if theres nothing to output
        if self.functions.is_empty() {
            return Ok(());
        }

        // find its capabilities
        let params = get_all_capability_params(conf, &module, &["CODE_WRITE"]);
        // build a hash map of which modules were allowed
        // to write which functions:
        let mut allowed_global_fn_map = std::collections::HashMap::<&String, Vec<&String>>::new();
        for (requestor, param) in &params["CODE_WRITE"] {
            if let Some(existing) = allowed_global_fn_map.get_mut(requestor) {
                existing.push(param);
            } else {
                allowed_global_fn_map.insert(requestor, vec![param]);
            }
        }

        // parse the stream back to a module so we can add stuff inside it if requested:
        let mut mod_def = syn::parse2::<ItemMod>(stream.clone())
            .map_err(|e| compiler_error(&format!("Failed to parse stream as module for {}\n{:?}", module.name, e)))?;
        let mut add_after = vec![];
        let contents = if let Some(contents) = &mut mod_def.content {
            &mut contents.1
        } else {
            return Err(compiler_error(&format!("Failed to find contents for module {}", module.name)));
        };

        for (requestor, map) in self.functions.iter() {
            if let Some(requestor_allowed) = allowed_global_fn_map.get(requestor) {
                for (sig, body) in map {
                    let (sig_type, signature) = match sig.split_once("|") {
                        Some(x) => x,
                        None => continue,
                    };
                    // first, parse the fn_signature
                    let full_fn = format!("{} {{ {} }}", signature, body);
                    let tokens = TokenStream::from_str(&full_fn)
                        .map_err(|e| compiler_error(&format!("Module {} provided invalid function signature '{}'\n{:?}", requestor, signature, e)))?;
                    let item_fn = syn::parse2::<ItemFn>(tokens.clone())
                        .map_err(|e| compiler_error(&format!("Module {} provided invalid function signature '{}'\n{:?}", requestor, signature, e)))?;
                    let sig = parse_fn_signature(&item_fn);
                    let fn_name = &sig.name;
                    // check if this requestor is allowed to write this function:
                    let desired_capability = if sig_type == "global" {
                        format!("fn_global:{}", fn_name)
                    } else {
                        format!("fn_module:{}", fn_name)
                    };
                    if !requestor_allowed.contains(&&desired_capability) {
                        return Err(compiler_error(&format!("Module {} attempted to write global function {} but no {} capability was defined", requestor, fn_name, desired_capability)));
                    }
                    if sig_type == "global" {
                        // add it after the module def:
                        add_after.push(tokens);
                    } else {
                        // otherwise, add it inside the module def:
                        contents.push(Item::Fn(item_fn));
                    }
                }
            } else {
                return Err(compiler_error(&format!("Module {} attempted to write a function, but no CODE_WRITE capability found", requestor)));
            }
        }

        // now put it back together
        let mut out_stream = mod_def.to_token_stream();
        out_stream.extend(add_after);
        *stream = out_stream;
        Ok(())
    }
}

impl L0CodeReader {
    pub fn initialize_capabilities(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        let mut params = get_all_capability_params(conf, &module, &["CODE_READ"]);
        // find all the requested function signatures across all modules:
        let mut function_signature_set = HashSet::new();
        let code_read_params = params.remove("CODE_READ").unwrap();
        for (dep, p) in code_read_params.iter() {
            if let Some((key, val)) = p.split_once(":") {
                match key {
                    "fn" => {
                        function_signature_set.insert(val.to_string());
                    },
                    x => {
                        return Err(compiler_error(&format!("Module {} requested READ_CODE capability of an unknown type '{}'", dep, x)));
                    }
                }
            } else {
                return Err(compiler_error(&format!("Module {} requested READ_CODE capability with an unknown syntax '{}'\nExpected to find something like 'fn:function_name'", dep, p)));
            } 
        }
        // get all function signatures of this lvl3 module that match all_fn_names
        let tokens = TokenStream::from_str(&module.contents)
            .map_err(|e| compiler_error(&format!("failed to parse module contents as a... module? {:?}", e)))?;
        let mut mod_def = syn::parse2::<ItemMod>(tokens)
            .map_err(|e| compiler_error(&format!("failed to parse module contents as a... module? {:?}", e)))?;

        let mut filler = FillCodeReader::default();
        filler.requested_fns = function_signature_set;
        iterate_mod_def_generic(
            &mut filler,
            &mut mod_def,
            &[set_functions],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        self.function_signatures = filler.function_signatures;

        Ok(())
    }
}

impl L0AppendFile {
    pub fn initialize_capabilities(&mut self, _conf: &mut HiraConfig, _module: &mut HiraModule2) -> Result<(), TokenStream> {
        // TODO: actually i dont think this is necessary.
        // for file operations i think itll be easier to be optimistic, and just let the writers
        // put data in this struct, and then we verify that its valid when we leave the wasm context.
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2, _stream: &mut TokenStream) -> Result<(), TokenStream> {
        let mut all_transient_deps = HashSet::new();
        module.visit_lvl3_dependency_names(&conf, &mut |dep| {
            all_transient_deps.insert(dep.to_string());
        });
        // collect all the files these modules are allowed to access:
        let mut all_allowed_files = HashSet::new();
        for dep in all_transient_deps.iter() {
            if let Some(dep_module) = conf.get_mod2(dep) {
                if let Some(allowed_files) = dep_module.get_capability_params("FILES") {
                    all_allowed_files.extend(allowed_files);
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

impl L0RuntimeCreator {
    pub fn initialize_capabilities(&mut self, _conf: &mut HiraConfig, _module: &mut HiraModule2) -> Result<(), TokenStream> {
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2, stream: &mut TokenStream) -> Result<(), TokenStream> {
        let mut params = get_all_capability_params(conf, &module, &["RUNTIME"]);
        let runtime_params = params.remove("RUNTIME").unwrap();
        for (runtime_name, runtime_info) in self.runtimes.drain() {
            for info in runtime_info.code_lines {
                let RuntimeInfo { creator, code, unique_line } = info;
                if !runtime_params.iter().any(|x| x.0 == *creator) {
                    return Err(compiler_error(&format!("Module '{}' requested to use runtime {} but no RUNTIME capability was found", creator, runtime_name)));
                }
                conf.add_to_runtime(runtime_name.to_string(), runtime_info.meta.clone(), code, unique_line);
            }
        }
        conf.output_runtimes(stream)?;
        Ok(())
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
            for (key, _) in kv_pairs.iter_mut() {
                if !module.has_output(key, conf) {
                    return Err(compiler_error(&format!("Detected output '{}' from intermediate module {} but this module did not specify such an output", key, mod_name)));
                }
            }
        }
        Ok(())
    }
    pub fn initialize_capabilities(&mut self, _conf: &mut HiraConfig, module: &mut HiraModule2) -> Result<(), TokenStream> {
        self.lvl3_module_name = module.name.clone();
        self.crate_name = std::env::var("CARGO_CRATE_NAME").unwrap_or("".to_string());
        Ok(())
    }
    pub fn apply_changes(&mut self, conf: &mut HiraConfig, module: &mut HiraModule2, stream: &mut TokenStream) -> Result<(), TokenStream> {
        // apply compiler error if any
        if !self.compiler_error_message.is_empty() {
            let add = format!("mod _hira_generated_error {{ fn _err() {{ compile_error!(r#\"{}\"#); }} }}", self.compiler_error_message);
            let add_tokens = TokenStream::from_str(&add)
                .map_err(|e| compiler_error(&format!("Failed to generate compiler error {:?}", e)))?;
            stream.extend(add_tokens);
        }
        // apply compiler warning if any
        if !self.compiler_warning_message.is_empty() {
            self.compiler_warning_message = format!("\n{}", self.compiler_warning_message);
            let add = format!("mod _hira_generated_warning {{ #[deprecated(note = r#\"{}\"#)]pub fn hira_generated_warning() {{}}\n fn _hira_use_warning() {{ hira_generated_warning() }} }}", self.compiler_warning_message);
            let add_tokens = TokenStream::from_str(&add)
                .map_err(|e| compiler_error(&format!("Failed to generate compiler warning {:?}", e)))?;
            stream.extend(add_tokens);
        }

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
        Self { shared_output_data: Default::default(), current_module_name: Default::default() }
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
        Self { current_module_name: Default::default(), data: Default::default() }
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
            compiler_warning_message: Default::default(),
            module_outputs: Default::default(),
            current_module_name: Default::default(),
            lvl3_module_name: Default::default(),
            crate_name: Default::default(),
        }
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
    /// this is the name of the user's module where they are referencing your module.
    /// eg: if your module is `my_dependency`, then the user's module name would be `mymod3`
    /// in this example:
    /// ```rust,ignore
    /// pub mod my_dependency { ... }
    /// 
    /// pub mod mymod3 {
    ///     pub fn config(input: &mut my_dependency::Input) {}
    /// }
    /// ```
    pub fn users_module_name(&self) -> String {
        self.lvl3_module_name.clone()
    }

    /// the name of the crate that will be compiled
    pub fn crate_name(&self) -> String {
        self.crate_name.clone()
    }

    pub fn compiler_error(&mut self, err: &str) {
        if self.compiler_error_message.is_empty() {
            self.compiler_error_message = err.to_string();
        }
    }

    pub fn compiler_warning(&mut self, msg: &str) {
        if self.compiler_warning_message.is_empty() {
            self.compiler_warning_message = msg.to_string();
        }
    }
}

#[output_and_stringify_basic_const(RUNTIME_IMPL)]
impl L0RuntimeCreator {
    pub fn new() -> Self {
        Self { current_module_name: Default::default(), runtimes: Default::default() }
    }
    /// add code to the entrypoint of the runtime you define. runtime_name will become the
    /// name of an executable, and code is a line of code in the main function. Note
    /// that code must not end in a semicolon, and must evaluate to ().
    /// for example this is valid `my_function()`, same as `my_error_function().expect("error")`
    /// but this would not be valid: `let x = 2;`
    pub fn add_to_runtime(&mut self, runtime_name: &str, code: String) {
        self.add_to_runtime_ex(runtime_name, code, RuntimeMeta { cargo_cmd: Default::default(), target: Default::default(), profile: Default::default() })
    }

    /// same as `add_to_runtime`, but the line of code is guaranteed to be unique in the main function.
    /// use this when your module can be potentially called many times, and you wish to ensure
    /// that your entrypoint only executes this line of code once.
    pub fn add_to_runtime_unique(&mut self, runtime_name: &str, code: String) {
        self.add_to_runtime_ex_unique(runtime_name, code, RuntimeMeta { cargo_cmd: Default::default(), target: Default::default(), profile: Default::default() })
    }

    /// same as `add_to_runtime`, but provide metadata for how this runtime should be compiled.
    /// for example can explicitly set a profile, can change the name of the program compiling,
    /// special targets, etc.
    pub fn add_to_runtime_ex(&mut self, runtime_name: &str, code: String, meta: RuntimeMeta) {
        self.add_to_runtime_ex_inner(runtime_name, code, meta, false)
    }

    pub fn add_to_runtime_ex_inner(&mut self, runtime_name: &str, code: String, meta: RuntimeMeta, unique_line: bool) {
        if let Some(existing) = self.runtimes.get_mut(runtime_name) {
            existing.code_lines.push(RuntimeInfo { creator: self.current_module_name.to_string(), code, unique_line });
        } else {
            let code_lines = vec![RuntimeInfo { creator: self.current_module_name.to_string(), code, unique_line }];
            self.runtimes.insert(runtime_name.to_string(), RuntimeData { code_lines, meta });
        }
    }

    /// same as `add_to_runtime_ex`, but the line of code will be unique.
    pub fn add_to_runtime_ex_unique(&mut self, runtime_name: &str, code: String, meta: RuntimeMeta) {
        self.add_to_runtime_ex_inner(runtime_name, code, meta, true)
    }
}

#[output_and_stringify_basic_const(CODE_READER_IMPL)]
impl L0CodeReader {
    pub fn new() -> Self {
        Self { current_module_name: Default::default(), function_signatures: Default::default() }
    }
    pub fn get_fn(&self, name: &str) -> Option<&FunctionSignature> {
        self.function_signatures.get(name)
    }
}

#[output_and_stringify_basic_const(CODE_WRITER_IMPL)]
impl L0CodeWriter {
    pub fn new() -> Self {
        Self { current_module_name: Default::default(), functions: Default::default() }
    }
    /// given a function signature and a function body, write
    /// this function inside the user's module. ie: this is internal
    /// to the user's module.
    pub fn write_internal_fn(&mut self, sig: String, body: String) {
        self.write_function(sig, body, "module");
    }
    fn write_function(&mut self, sig: String, body: String, prefix: &str) {
        if !self.functions.contains_key(&self.current_module_name) {
            self.functions.insert(self.current_module_name.to_string(), Default::default());
        }
        if let Some(map) = self.functions.get_mut(&self.current_module_name) {
            map.insert(format!("{}|{}", prefix, sig), body);
        }
    }
    /// given a function signature and a function body, write
    /// this function outside the user's module. ie: this will be
    /// callable globally
    pub fn write_global_fn(&mut self, sig: String, body: String) {
        self.write_function(sig, body, "global");
    }
}

#[output_and_stringify_basic_const(LIBRARY_OBJ_IMPL)]
impl LibraryObj {
    // this is used by the code generator to ensure
    // that when each module's config function is called, this
    // sets the name such that if that module calls
    // "set_output", then it gets properly set into the module_outputs field
    #[doc(hidden)]
    pub fn set_current_module(&mut self, name: &str) {
        self.l0_append_file.current_module_name = name.to_string();
        self.l0_core.current_module_name = name.to_string();
        self.l0_kv_reader.current_module_name = name.to_string();
        self.l0_code_writer.current_module_name = name.to_string();
        self.l0_runtime_creator.current_module_name = name.to_string();
    }

    // if adding a new l0 functionality,
    // remember to add a `output_and_stringify_basic_const`
    // and add the stringified impl section to `get_include_string`
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            l0_kv_reader: L0KvReader::new(),
            l0_core: L0Core::new(),
            l0_append_file: L0AppendFile::new(),
            l0_code_reader: L0CodeReader::new(),
            l0_code_writer: L0CodeWriter::new(),
            l0_runtime_creator: L0RuntimeCreator::new(),
        }
    }
}

pub fn get_include_string() -> &'static [&'static str] {
    &[
        LIBRARY_OBJ_IMPL, FILE_IMPL, CORE_IMPL, KV_IMPL, CODE_READER_IMPL,
        CODE_WRITER_IMPL, RUNTIME_IMPL,
    ]
}
