use std::collections::{HashMap, HashSet};


use serde::{Serialize, Deserialize};

use proc_macro2::TokenStream;
use quote::{ToTokens};

use crate::parsing::{remove_surrounding_quotes, parse_as_module_item, iterate_mod_def, get_ident_string, iterate_item_tree, parse_module_name_from_use_tree, iterate_tuples};
use crate::{wasm_types::*, level0::*};


use super::HiraConfig;
use super::parsing::{default_stream, compiler_error, iterate_expr_for_strings, DependencyTypeName};
use super::use_hira_config;

pub const FN_ENTRYPOINT_NAME: &'static str = "wasm_entrypoint";
pub const LIB_OBJ_TYPE_NAME: &'static str = "LibraryObj";
pub const REQUIRED_CRATES_NAME: &'static str = "REQUIRED_CRATES";
pub const REQUIRED_HIRA_MODS_NAME: &'static str = "REQUIRED_HIRA_MODULES";
pub const HIRA_MOD_NAME_NAME: &'static str = "HIRA_MODULE_NAME";
pub const EXPORT_ITEM_NAME: &'static str = "ExportType";
pub const CAPABILITY_PARAMS_NAME: &'static str = "CAPABILITY_PARAMS";


#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum ModuleLevel {
    /// built into hira itself
    Level1,
    /// Can depend on multiple Level1 and Level2 modules
    Level2,
    /// Can only depend on 1 single Level 2 module. Can specify `mod outputs`
    Level3,
}

impl Default for ModuleLevel {
    fn default() -> Self {
        Self::Level1
    }
}


#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum OutputType {
    /// corresponds to doing:
    /// ```rust,ignore
    /// mod outputs {
    ///     use some_lvlv2::outputs::*;
    /// }
    /// ```
    /// In this case we just store the name of the lvl2 dependency
    AllFromModule(String),
    /// corresponds to doing:
    /// ```rust,ignore
    /// mod outputs {
    ///     use some_lvlv2::outputs::{A, B, C};
    /// }
    /// ```
    /// in this case we specify (some_lvl2, A), (some_lvl2, B), (some_lvl2, C)
    SpecificFromModule(String, String),
    /// corresponds to doing:
    /// ```rust,ignore
    /// mod outputs {
    ///     const SOME_OUTPUT: &'str = "hi";
    /// }
    /// ```
    /// Only lvl2 modules are allowed to specify specific constant values.
    /// These can then be referenced by lvl3 modules explicitly. The string is just
    /// the name of the constant ident, and then the value. and it is implied that the dependency is self.
    SpecificConst(String, String),
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct HiraModule2 {
    pub name: String,
    pub contents: String,
    pub config_fn_signature_inputs: Vec<String>,
    pub is_pub: bool,
    pub input_struct: String,
    /// List of names of fields + module names that this
    /// module depends on. For example can be a single module
    /// and all of its dependencies "use X::*", or can be
    /// specific fields such as "use X::{Y, Z}". key is the name of the module,
    /// values are the list of fields from that module. if we find
    /// "use X::*;" and "use X::{Y, Z}", we only use the "*" import.
    /// modules that specify "use X"
    /// Failure to resolve a dependency results in a compilation failure
    /// and a recommendation to use the hira compiler tool instead.
    /// example: if X comes after this current module, then hira in proc-macro
    /// mode cannot know anything about X, and therefore fails.
    pub dependencies: HashMap<String, Result<Vec<String>, String>>,

    pub level: ModuleLevel,

    /// whereas `dependencies` tracks logical dependencies, `compile_dependencies`
    /// tracks actual dependencies required for compiling this as a wasm module.
    /// This field is inferred based on the config function signature, not the use statements.
    /// This field is set after parsing, as it requires verification that modules exist
    pub compile_dependencies: Vec<DependencyTypeName>,

    /// if this is a level3 module, then we set this field to be the name of the level2
    /// module that this module referenced in its config function
    pub lvl3_module_depends_on: Option<String>,

    /// I was tempted to overload the term 'dependency' even more...
    /// These extern crates represent anytime the user added `extern crate X` to
    /// their module. These values are then used to pass the names of dependency
    /// crates that should be compiled prior to compiling the user's wasm. This enables
    /// using arbitrary 3rd party dependencies within wasm!
    pub extern_crates: Vec<String>,

    /// List of names of fields that this module outputs to be
    /// used by other modules.
    /// must be individual items inside
    /// "pub mod outputs { ... }"
    /// or simply "pub mod outputs { use lvl2module::outputs::* }"
    /// Outputs must be statically defined, ie: specific fields w/ names and types
    /// therefore something like "use X::*" must ensure that X can be resolved at
    /// the time we are processing this module, failure to resolve results in
    /// a compilation failure
    pub outputs: Vec<OutputType>,

    /// after the wasm module runs, we have the final output key/values.
    /// we set these in memory such that other modules that depend on these values can
    /// reference them.
    pub resolved_outputs: HashMap<String, String>,

    /// a map of the name of the capability to a list of values
    /// that this module needs for that capability. it's generic on purpose
    /// such that level0 modules can expand on top of this functionality
    /// and can define their own custom keywords/semantics
    pub capability_params: HashMap<String, Vec<String>>,
}

impl HiraModule2 {
    pub fn get_cached_json_path(module_name: &str, cache_dir: &str) -> String {
        format!("{}/{}.json", cache_dir, module_name)
    }
    pub fn cache_to_disk(&self, cache_dir: &str) {
        // ensure the directory exists:
        let _ = std::fs::create_dir_all(cache_dir);
        let write_to = Self::get_cached_json_path(&self.name, cache_dir);
        if let Ok(serialized)  = serde_json::to_string(&self) {
            let _ = std::fs::write(write_to, serialized);
        }
    }
    pub fn load_from_cache(cache_dir: &str, name: &str) -> Result<Self, TokenStream> {
        let file_path = Self::get_cached_json_path(name, cache_dir);
        let err = |e| {
            compiler_error(&format!("Failed to load dependant module '{}' from cache file {}\n{:?}", name, file_path, e))
        };
        let contents = std::fs::read_to_string(&file_path)
            .map_err(|e| err(e))?;
        let obj: Self = serde_json::from_str(&contents)
            .map_err(|e| err(e.into()))?;
        Ok(obj)
    }
    pub fn get_capability_params(&self, capability_name: &str) -> Option<&Vec<String>> {
        if let Some(list) = self.capability_params.get(capability_name) {
            return Some(list);
        }
        None
    }
    pub fn get_dependencies(&self, s: &str) -> Option<Vec<String>> {
        let entry = self.dependencies.get(s)?;
        match entry {
            // this is a renamed entry
            Err(renamed) => {
                self.get_dependencies(&renamed)
            }
            Ok(out) => {
                Some(out.clone())
            }
        }
    }

    pub fn visit_dependencies_recursively(name: &str, conf: &HiraConfig, cb: &mut impl FnMut(&str)) {
        if let Some(module) = conf.get_mod2(name) {
            for dep in module.compile_dependencies.iter() {
                match dep {
                    DependencyTypeName::Mod1Or2(mod_name) => {
                        cb(&mod_name);
                        Self::visit_dependencies_recursively(&mod_name, conf, cb);
                    }
                    // these are ignored as theres nothing to visit
                    DependencyTypeName::Library(_) => {}
                }
            }
        }
    }

    pub fn visit_lvl3_dependency_names(&self, conf: &HiraConfig, cb: &mut impl FnMut(&str)) {
        if let Some(dep_name) = &self.lvl3_module_depends_on {
            cb(dep_name);
            Self::visit_dependencies_recursively(&dep_name, conf, cb);
        }
    }

    pub fn visit_lvl2_dependency_names(&self, conf: &HiraConfig, cb: &mut impl FnMut(&str)) {
        for dep in self.compile_dependencies.iter() {
            if let DependencyTypeName::Mod1Or2(dep_name) = dep {
                cb(&dep_name);
                Self::visit_dependencies_recursively(dep_name, conf, cb);
            }
        }
    }

    pub fn has_output(&self, k: &str, conf: &HiraConfig) -> bool {
        for output in self.outputs.iter() {
            match output {
                OutputType::SpecificConst(c, _) => {
                    if c == k { return true }
                }
                OutputType::AllFromModule(mod_name) => {
                    if let Some(module) = conf.get_mod2(&mod_name) {
                        if module.has_output(k, conf) {
                            return true;
                        }
                    }
                }
                OutputType::SpecificFromModule(_, new_key) => {
                    if new_key == k { return true }
                }
                
            }
        }
        false
    }

    pub fn assert_level_3_and_set_depends_on(&mut self) -> Result<(), TokenStream> {
        let mut has_l2_dep = None;
        for dep in self.compile_dependencies.iter() {
            match dep {
                DependencyTypeName::Mod1Or2(x) => {
                    has_l2_dep = Some(x);
                }
                DependencyTypeName::Library(x) => {
                    return Err(compiler_error(&format!("Detected {} as {:?}, but found it attempting to use {} in its config function. Only Level2 modules are allowed to use Level1 functionality in the config function", self.name, self.level, x)));
                }
            }
        }
        if self.config_fn_signature_inputs.len() != 1 {
            return Err(compiler_error(&format!("Detected {} as {:?}, but its config function signature has more than 1 input", self.name, self.level)));
        }
        let s = self.level3_get_depends_on(has_l2_dep)?;
        self.lvl3_module_depends_on = Some(s);
        Ok(())
    }

    pub fn level3_get_depends_on(&self, opt: Option<&String>) -> Result<String, TokenStream> {
        let l2_dep_name = opt
        .ok_or_else(|| {
            compiler_error(&format!("Detected {} as {:?}, but failed to find a level2 module's input in the config function signature", self.name, self.level))
        })?;
        Ok(l2_dep_name.to_string())
    }

    /// given a level2 module ,iterate through all its dependencies
    /// and verify the hira config has them loaded, and if not:
    /// try to load them from cache
    pub fn verify_dependencies2_exist_or_load(&mut self, conf: &mut HiraConfig) -> Result<(), TokenStream> {
        let mut all_transient_deps = HashSet::new();
        self.visit_lvl2_dependency_names(&conf, &mut |dep| {
            all_transient_deps.insert(dep.to_string());
        });
        // iterate over all dependencies and try to load them if they dont exist
        for dep in all_transient_deps {
            if conf.modules2.contains_key(&dep) { continue; }
            // havent loaded this dependency yet. try to load from cache:
            let mut loaded = Self::load_from_cache(&conf.module_cache_directory, &dep)?;
            loaded.verify_dependencies2_exist_or_load(conf)?;
            conf.modules2.insert(loaded.name.to_string(), loaded);
        }
        Ok(())
    }

    /// given a level3 module, iterate through all its dependencies
    /// and verify the hira config has them loaded, and if not:
    /// try to load them from cache
    pub fn verify_dependencies_exist_or_load(&mut self, conf: &mut HiraConfig) -> Result<(), TokenStream> {
        // find all transient modules that might have requested code read abilities
        let mut all_transient_deps = HashSet::new();
        self.visit_lvl3_dependency_names(&conf, &mut |dep| {
            all_transient_deps.insert(dep.to_string());
        });
        // iterate over all dependencies and try to load them if they dont exist
        for dep in all_transient_deps {
            if conf.modules2.contains_key(&dep) { continue; }
            // havent loaded this dependency yet. try to load from cache:
            let mut loaded = Self::load_from_cache(&conf.module_cache_directory, &dep)?;
            loaded.verify_dependencies2_exist_or_load(conf)?;
            conf.modules2.insert(loaded.name.to_string(), loaded);
        }
        Ok(())
    }

    pub fn verify_config_signature(&mut self, conf: &mut HiraConfig) -> Result<(), TokenStream> {
        for s in self.config_fn_signature_inputs.iter() {
            if s == "& mut Input" {
                continue;
            }
            // if i == 0 && s != "& mut Input" {
            //     return Err(compiler_error(&format!("First input param to config function must be your own Input item. Expected `fn config(&mut Input, ...)`, Found `fn config({}, ...)`", s)));
            // }
            // remove the &mut
            let after_mut = s.replace("& mut", "");
            let after_mut = after_mut.trim();
            // the only valid options are:
            // dependency structs which always are called Input,
            // and library capabilities which always start with L0.
            if after_mut.ends_with("Input") {
                // parse out the module name
                if let Some((first, _)) = s.split_once("::") {
                    let module_name = first.replace("& mut", "");
                    self.compile_dependencies.push(DependencyTypeName::Mod1Or2(module_name.trim().to_string()));
                }
            } else if after_mut.starts_with("L0") {
                self.compile_dependencies.push(DependencyTypeName::Library(after_mut.trim().to_string()));
            }
        }
        // if this has more than 1 signature input, and its not a L1 module, then it is a L2 module
        //    (because we know its not an L1 module, and L3 modules can only have 1 signature input)
        // if this module does not have an input struct, it MUST be a L3 module
        //    (because all other types of modules must specify their input shape)
        // otherwise we default to assume its level2, but we perform validation afterwards
        if self.config_fn_signature_inputs.len() > 1 {
            self.level = ModuleLevel::Level2;
        } else if self.input_struct.is_empty() {
            self.level = ModuleLevel::Level3;
        } else {
            self.level = ModuleLevel::Level2;
        }

        if self.level != ModuleLevel::Level3 && !self.is_pub {
            return Err(compiler_error(
                &format!("Detected module {} as {:?}, but it is not marked public. Level2 modules must be public", self.name, self.level)
            ));
        }

        if self.level == ModuleLevel::Level3 {
            self.assert_level_3_and_set_depends_on()?;
            self.verify_dependencies_exist_or_load(conf)?;
        }

        // TODO: add capability checks, eg: module level2s arent allowed to use outputs,
        // module level3s are only allowed to have 1 input param,
        // module level1s cannot depend on level2s, etc.

        // TODO:
        // add check for input struct,
        // ensure level3 does not have one.
        // ensure other levels DO have one, and ensure it has a Default method
        // scan its attributes for (Derive(Default)), impl w/ a default() signature, etc.

        // verify the shape of outputs is valid:
        if !self.outputs.is_empty() {
            match self.level {
                ModuleLevel::Level3 => {
                    // it should be guaranteed at this point that we know the l2 dependency
                    // but just in case we unwrap w/ default
                    let default = "".to_string();
                    let l2_dependency = self.lvl3_module_depends_on.as_ref().unwrap_or(&default);
                    // ensure L3 modules can only specify use statements
                    // also ensure that L3 module outputs only depend on 1 level 2 module
                    // and ensure that this level 2 module is the same one in its input config
                    for output in self.outputs.iter() {
                        match output {
                            OutputType::SpecificConst(_, _) => {
                                return Err(compiler_error(&format!("Detected module {} as {:?}, but in its `mod outputs` section there are const statements. Only Level2 modules can specify const statements in its outputs section", self.name, self.level)));
                            }
                            OutputType::AllFromModule(mod_name) | OutputType::SpecificFromModule(mod_name, _) => {
                                if mod_name != l2_dependency {
                                    return Err(compiler_error(&format!("Detected module {} as {:?}. Its `mod outputs` section contains use statements from other modules. Expected to only see use statements from Level2 module {}, but found {}. Level3 modules can only specify outputs that exist in the corresponding Level2 module", self.name, self.level, l2_dependency, mod_name)));
                                }
                            }
                        }
                    }
                }
                // its impossible for this to be a level1
                _ => {}
            }
        }

        Ok(())
    }
}

/// corresponds to the main hira_mod! macro
pub fn hira_mod2(mut stream: TokenStream, mut _attr: TokenStream) -> TokenStream {
    let mut out = Err(default_stream());
    let out_ref = &mut out;
    use_hira_config(|conf| {
        let stream = std::mem::take(&mut stream);
        // let attr = std::mem::take(&mut attr);
        *out_ref = hira_mod2_inner(conf, stream);
    });
    match out {
        Ok(o) => o,
        Err(e) => e,
    }
}

pub fn get_all_extern_crates(conf: &mut HiraConfig, module: &mut HiraModule2) -> Vec<String> {
    let mut all_externs = HashSet::new();
    for ext in module.extern_crates.iter() {
        all_externs.insert(ext);
    }
    module.visit_lvl3_dependency_names(conf, &mut |name| {
        if let Some(dep_mod) = conf.get_mod2(name) {
            for ext in dep_mod.extern_crates.iter() {
                all_externs.insert(ext);
            }
        }
    });
    all_externs.drain().map(|x| x.to_string()).collect()
}

pub fn should_compile() -> bool {
    if let Ok(val) = std::env::var("RUST_BACKTRACE") {
        // rust-analyzer always outputs short:
        // https://github.com/rust-lang/rust-analyzer/blob/master/crates/rust-analyzer/src/bin/main.rs#L110
        // if that changes ^ we're in trouble.
        if val == "short" { return false }
    }
    true
}

pub fn hira_mod2_inner(conf: &mut HiraConfig, mut stream: TokenStream) -> Result<TokenStream, TokenStream> {
    let mut module = parse_module_from_stream(stream.clone())?;
    module.verify_config_signature(conf)?;

    // only level3 modules get compiled into wasm
    // all other modules get compiled as dependencies for a level3 module
    // but theres no point to compile them all individually
    if module.level != ModuleLevel::Level3 {
        // cache it in case this module is needed as a dependency
        // in another crate
        module.cache_to_disk(&conf.module_cache_directory);
        conf.modules2.insert(module.name.clone(), module);
        return Ok(stream);
    }
    // originally i had the idea that itd be nice to get compiler errors
    // as you type in your editor, so you can get a quicker feedback loop.
    // this means on every file save, your typehint program would run cargo check
    // which would invoke hira, which would compile wasm, run it, and return the output.
    // this, however, takes way too long to be considered quick, particularly for
    // hira modules that have big dependencies like serde.
    // instead, what i've decided to do is to try to not compile any wasm if
    // we detect that we're being invoked from cargo check. this isn't a foolproof method
    // but a quick/dirty way is to check if we have RUST_BACKTRACE=full or not (cargo build
    // uses full, whereas cargo check uses short by default)
    if !should_compile() {
        return Ok(stream);
    }

    let codes = get_wasm_code_to_compile2(conf, &module)?;
    let extern_dependencies = get_all_extern_crates(conf, &mut module);
    let mut pass_this = LibraryObj::new();
    pass_this.initialize_capabilities(conf, &mut module)?;

    let mut lib_obj = get_wasm_output(
        &conf.wasm_directory,
        &codes,
        &extern_dependencies,
        &pass_this
    ).unwrap_or_default();

    lib_obj.apply_changes(conf, &mut module, &mut stream)?;
    conf.modules2.insert(module.name.clone(), module);
    Ok(stream)
}

pub fn set_config_fn_sig(module: &mut HiraModule2, item: &mut syn::ItemFn) {
    let sig = &item.sig;
    let fn_name = get_ident_string(&sig.ident);
    if fn_name != "config" { return }
    for input in &sig.inputs {
        let push_s = match input {
            syn::FnArg::Receiver(_) => "self".to_string(),
            syn::FnArg::Typed(x) => {
                x.ty.to_token_stream().to_string()
            }
        };
        module.config_fn_signature_inputs.push(push_s);
    }
}

pub fn set_capability_params(module: &mut HiraModule2, item: &mut syn::ItemConst) {
    if item.ident.to_string() != CAPABILITY_PARAMS_NAME {
        return;
    }
    iterate_tuples(&*item.expr, &mut |key, val| {
        if !module.capability_params.contains_key(&key) {
            module.capability_params.insert(key.to_string(), vec![]);
        }
        if let Some(list) = module.capability_params.get_mut(&key) {            
            iterate_expr_for_strings(val, |a| {
                list.push(a);
            });
        }
    });
}

pub fn set_input_item_struct(module: &mut HiraModule2, item: &mut syn::ItemStruct) {
    let struct_name = get_ident_string(&item.ident);
    if struct_name == "Input" {
        module.input_struct = item.to_token_stream().to_string();
    }
}

pub fn set_dep_inner(deps: &mut HashMap<String, Result<Vec<String>, String>>, dep_name: &String, field: String) {
    match deps.get_mut(dep_name) {
        Some(Ok(existing_vec)) => {
            match (existing_vec.contains(&"*".to_string()), field == "*") {
                // the existing vec is already just *, so ignore
                (true, true) => {},
                // existing vec is already *, but we want to add
                // something that isn't *. ignore, because * overrides everything else
                (true, false) => {},
                // we want to set existing vec to * and override existing entries
                (false, true) => {
                    existing_vec.clear();
                    existing_vec.push(field);
                }
                // not using wildcards, just push
                (false, false) => {
                    existing_vec.push(field);
                }
            }
        }
        None => {
            deps.insert(dep_name.to_string(), Ok(vec![field]));
        }
        // should not be possible
        _ => {}
    }
}

pub fn set_dep(deps: &mut HashMap<String, Result<Vec<String>, String>>, dep_name: &String, renamed: &String, field: String) {
    if dep_name != renamed {
        deps.insert(renamed.to_string(), Err(dep_name.to_string()));
    }
    set_dep_inner(deps, dep_name, field);
}

pub fn set_dependencies_recursively(deps: &mut HashMap<String, Result<Vec<String>, String>>, tree: &syn::UseTree) {
    let mut past_names = vec![];
    iterate_item_tree(&mut past_names, tree, &mut |names, renamed, wildcard| {
        // first, find the actual module name
        let (mod_name, specific_import) = match parse_module_name_from_use_tree(names) {
            Some(a) => a,
            None => return,
        };
        // if the last part is a wildcard, then we want all imports.
        // also: if there is no specific import, this means the last component
        // is "outputs", and then we also want to use a wildcard
        let last_part = match (wildcard, specific_import) {
            (true, _) => "*".to_string(),
            (false, None) => "*".to_string(),
            (false, Some(x)) => x.to_string(),
        };

        // dont allow "use X::outputs::something as abc"
        // renaming only allowed for "use X::outputs as x_outputs"
        if last_part != "*" && renamed.is_some() {
            return;
        }
        let renamed = match renamed {
            Some(x) => x,
            None => mod_name.to_string()
        };
        set_dep(deps, mod_name, &renamed, last_part);
    });
}

/// TODO: how to differentiate between hira dependencies like another hira module
/// and a normal crate/module that this module wants to use...
pub fn set_dependencies(module: &mut HiraModule2, item: &mut syn::ItemUse) {
    let mut deps = std::mem::take(&mut module.dependencies);
    set_dependencies_recursively(&mut deps, &item.tree);
    module.dependencies = deps;
}

pub fn set_extern_crates(module: &mut HiraModule2, item: &mut syn::ItemExternCrate) {
    let name = get_ident_string(&item.ident);
    module.extern_crates.push(name);
}

pub fn set_outputs(module: &mut HiraModule2, item: &mut syn::ItemMod) {
    let name = get_ident_string(&item.ident);
    if name != "outputs" { return; }
    match item.vis {
        // ignore non-public output modules. this will be caught in verification
        // and show an error to the user if they didnt mark their outputs as pub
        syn::Visibility::Restricted(_) | syn::Visibility::Inherited => {
            return
        }
        _ => {}
    }
    let mut default_vec = vec![];
    for item in item.content.as_mut().map(|x| &mut x.1).unwrap_or(&mut default_vec) {
        if let syn::Item::Const(c) = item {
            let name = get_ident_string(&c.ident);
            // TODO: actually check the type.. we should enforce that its a string.
            let mut val = c.expr.to_token_stream().to_string();
            remove_surrounding_quotes(&mut val);
            module.outputs.push(OutputType::SpecificConst(name, val));
            continue;
        }
        if let syn::Item::Use(u) = item {
            let mut names = vec![];
            iterate_item_tree(&mut names, &u.tree, &mut |paths, _, wildcard| {
                let (mod_name, specific_import) = match parse_module_name_from_use_tree(paths) {
                    Some(x) => x,
                    None => return,
                };
                match (wildcard, specific_import) {
                    (true, _) => {
                        module.outputs.push(OutputType::AllFromModule(mod_name.to_string()));
                    }
                    (false, None) => {
                        // this corresponds to "use other_module::outputs"
                        // this shouldnt be allowed in this context. so we just ignore it.
                    }
                    (false, Some(specific)) => {
                        module.outputs.push(OutputType::SpecificFromModule(mod_name.to_string(), specific.to_string()));
                    }
                }
            });
        }
    }
}

pub fn parse_module_from_stream(stream: TokenStream) -> Result<HiraModule2, TokenStream> {
    let mut mod_def = parse_as_module_item(stream)?;
    let mut hira_mod = HiraModule2::default();
    iterate_mod_def(
        &mut hira_mod,
        &mut mod_def,
        &[set_config_fn_sig],
        &[set_input_item_struct],
        &[set_dependencies],
        &[set_outputs],
        &[set_capability_params],
        &[set_extern_crates],
    );
    Ok(hira_mod)
}


#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use syn::ItemConst;

    use crate::e2e_tests::assert_contains_str;

    use super::*;

    #[test]
    fn basic_mod2_parsing_works() {
        let code = r#"
        mod hello_world {
            // these should be represented the same way:
            use crate::dependency_b::outputs;
            use crate::dependency_a::outputs::*;
            // groups work:
            use crate::{
                // xyz should resolve to somedep1
                somedep1::outputs as xyz,
                // somedep2 should have explicit outputs A1, and A2
                somedep2::{
                    outputs::A1,
                    outputs::A2,
                    // should not allow renaming specific fields
                    outputs::A3 as somethingelse,
                }
            };
            // ignored:
            use some_library;


            pub struct Input {
                pub a: u32,
            }

            mod outputs {
                pub const HEY: &'static str = "dsa";
            }

            pub fn config(input: &mut Input) {

            }
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        assert_eq!(module.name, "hello_world");
        assert_eq!(module.config_fn_signature_inputs.len(), 1);
        assert_eq!(module.config_fn_signature_inputs[0], "& mut Input");
        // println!("{:#?}", module.dependencies);
        assert!(module.dependencies.contains_key("dependency_a"));
        assert!(module.dependencies.contains_key("dependency_b"));
        assert!(module.dependencies.contains_key("somedep1"));
        assert!(module.dependencies.contains_key("xyz"));
        assert!(module.dependencies.contains_key("somedep2"));
        assert!(!module.dependencies.contains_key("some_library"));
        assert_eq!(module.dependencies["somedep2"], Ok(vec!["A1".to_string(), "A2".to_string()]));
        assert!(module.input_struct.contains("pub a"));
        assert!(module.input_struct.contains("pub struct Input"));
    }

    #[test]
    fn mod2_can_detect_extern_crates() {
        let code = r#"
        mod hello_world {
            extern crate some_dependency;

            mod outputs {
                pub const HEY: &'static str = "dsa";
            }
            pub struct Input { pub a: u32 }
            pub fn config(input: &mut Input) {}
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        assert_eq!(module.name, "hello_world");
        assert_eq!(module.extern_crates.len(), 1);
        assert_eq!(module.extern_crates[0], "some_dependency");
    }

    #[test]
    fn mod2_file_permissions_get_set_correctly() {
        let code = r#"pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[("FILES", &["hello.txt"])];"#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let mut item = syn::parse2::<ItemConst>(stream).unwrap();
        let mut module = HiraModule2::default();
        set_capability_params(&mut module, &mut item);
        assert_eq!(module.capability_params["FILES"][0], "hello.txt");
    }

    #[test]
    fn mod2_verify_works() {
        let code = r#"
        pub mod hello_world {
            pub struct Input {
                pub a: u32,
            }
            mod outputs {
                pub const HEY: &'static str = "dsa";
            }
            pub fn config(input: &mut Input) {

            }
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let mut module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        let mut conf = HiraConfig::default();
        let out = module.verify_config_signature(&mut conf);
        assert!(out.is_ok());
    }

    #[test]
    fn mod2_multiple_params_works() {
        let code = r#"
        pub mod hello_world {
            pub struct Input {
                pub a: u32,
            }
            mod outputs {
                pub const HEY: &'static str = "dsa";
            }
            pub fn config(input: &mut Input, other: &mut other::Input, libobj: &mut L0Reader, somethingelse: &mut hello::Input) {

            }
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let mut module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        let mut conf = HiraConfig::default();
        let out = module.verify_config_signature(&mut conf);
        assert!(out.is_ok());
        assert_eq!(module.compile_dependencies.len(), 3);
        assert!(module.compile_dependencies.contains(&DependencyTypeName::Mod1Or2("other".to_string())));
        assert!(module.compile_dependencies.contains(&DependencyTypeName::Mod1Or2("hello".to_string())));
        assert!(module.compile_dependencies.contains(&DependencyTypeName::Library("L0Reader".to_string())));
    }

    #[test]
    fn mod2_non_lvl3_must_be_pub() {
        let code = r#"
        mod hello_world {
            pub struct Input { pub a: u32 }
            pub fn config(input: &mut Input) {}
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let mut module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        let mut conf = HiraConfig::default();
        let out = module.verify_config_signature(&mut conf);
        assert!(out.is_err());
        let err = out.err().unwrap().to_string();
        assert!(err.contains("must be public"));
    }

    #[test]
    fn mod2_lvl3_outputs_can_only_depend_on_its_corresponding_l2_mod() {
        let code = r#"
        mod hello_world {
            pub mod outputs {
                use some_other_dep::outputs::*;
            }
            pub fn config(input: &mut l2_dep::Input) {}
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let mut module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        let mut conf = HiraConfig::default();
        conf.modules2.insert("l2_dep".to_string(), Default::default());
        let out = module.verify_config_signature(&mut conf);
        assert!(out.is_err());
        let err = out.err().unwrap().to_string();
        assert_contains_str(err, "Expected to only see use statements from Level2 module l2_dep, but found some_other_dep");
    }

    #[test]
    fn mod2_outputs_work() {
        let code = r#"
        mod hello_world {
            pub struct Input { pub a: u32 }
            pub fn config(input: &mut Input) {}

            // it is invalid to have an outputs section like this:
            // this would fail verification. but for the purpose of this test
            // we put all cases in 1 module. this works
            // because this test case doesnt run verification
            pub mod outputs {
                use other_module::outputs; // should be ignored
                use something::outputs::specific;
                use apples::outputs::*;
                pub const HELLO: &'static str = "dsa";
            }
        }
        "#;
        let stream = TokenStream::from_str(code).expect("Failed to parse test case as token stream");
        let module = parse_module_from_stream(stream).expect("failed to parse test case as module");
        assert_eq!(module.outputs[0], OutputType::SpecificFromModule("something".to_string(), "specific".to_string()));
        assert_eq!(module.outputs[1], OutputType::AllFromModule("apples".to_string()));
        assert_eq!(module.outputs[2], OutputType::SpecificConst("HELLO".to_string(), "dsa".to_string()));
    }
}
