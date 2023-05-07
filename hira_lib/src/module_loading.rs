use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use proc_macro2::TokenStream;
use quote::{quote, format_ident, ToTokens};
use syn::{parse_file, ItemUse};

use crate::parsing::{remove_surrounding_quotes, get_input_type, parse_callback_required_module, extract_default_attr, parse_as_module_item, iterate_mod_def, get_ident_string, iterate_item_tree, parse_module_name_from_use_tree};
use crate::wasm_types::*;
use wasm_type_gen::*;

use super::HiraConfig;
use super::parsing::{default_stream, compiler_error, iterate_file, iterate_expr_for_strings, get_list_of_strings, DependencyTypeName};
use super::use_hira_config;

pub const FN_ENTRYPOINT_NAME: &'static str = "wasm_entrypoint";
pub const LIB_OBJ_TYPE_NAME: &'static str = "LibraryObj";
pub const REQUIRED_CRATES_NAME: &'static str = "REQUIRED_CRATES";
pub const REQUIRED_HIRA_MODS_NAME: &'static str = "REQUIRED_HIRA_MODULES";
pub const HIRA_MOD_NAME_NAME: &'static str = "HIRA_MODULE_NAME";
pub const EXPORT_ITEM_NAME: &'static str = "ExportType";

#[derive(Debug)]
pub enum LoadedFrom {
    /// if user provides just a module name, it's implied that
    /// there is such a file in the hira/modules folder
    Implied,
    /// user either provided a URL, or a hira module namespace:name:
    Remote,
    /// a file that was either specified via absolute, or relative path
    ExternalFile,
}

#[derive(Debug)]
pub struct HiraModule {
    pub name: String,
    pub loaded_from: LoadedFrom,
    pub contents: String,

    pub required_hira_modules: Vec<String>, // syn::Item::Const
    pub required_crates: Vec<String>, // syn::Item::Const
    pub export_items: HashMap<String, String>, // anything pub
    pub primary_export_item: String, // syn::Item::Struct
    pub entrypoint_fn: Option<String>, // syn::Item::Fn
}

#[derive(Debug, PartialEq, Clone, Copy)]
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


#[derive(Debug, PartialEq)]
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

#[derive(Default, Debug)]
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
    /// This field is set after parsing, as it requires verification that modules exist
    pub compile_dependencies: Vec<DependencyTypeName>,

    /// if this is a level3 module, then we set this field to be the name of the level2
    /// module that this module referenced in its config function
    pub lvl3_module_depends_on: Option<String>,

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
}

impl HiraModule2 {
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

    pub fn has_output(&self, k: &str) -> bool {
        for output in self.outputs.iter() {
            match output {
                OutputType::SpecificConst(c, _) => {
                    if c == k { return true }
                }
                // TODO: should L2 modules be allowed to do
                // pub mod outputs { use other_module::outputs::*; }
                // ?
                // if so, need to recurse and do more lookups...
                _ => {}
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

    pub fn verify_config_signature(&mut self) -> Result<(), TokenStream> {
        let mut has_l0_deps = false;
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
                has_l0_deps = true;
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
                ModuleLevel::Level2 => {
                    // ensure L2 modules can only specify specific const outputs
                    if self.outputs.iter().any(|x| if let OutputType::SpecificConst(_, _) = x { false } else { true }) {
                        return Err(compiler_error(&format!("Detected module {} as {:?}, but in its `mod outputs` section there are use statements. Only Level3 modules can specify use statements in its outputs section", self.name, self.level)));
                    }
                }
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

impl Default for HiraModule {
    fn default() -> Self {
        Self {
            name: Default::default(),
            loaded_from: LoadedFrom::Implied,
            contents: Default::default(),
            required_crates: Default::default(),
            export_items: Default::default(),
            primary_export_item: Default::default(),
            required_hira_modules: Default::default(),
            entrypoint_fn: Default::default(),
        }
    }
}

impl HiraModule {
    pub fn to_token_stream(&self, include_super: bool) -> TokenStream {
        let module_name = format_ident!("{}", self.name);
        let mut list = self.export_items.iter().map(|(_, v)| {
            v.clone()
        }).collect::<Vec<String>>();
        // we want it to be deterministic
        list.sort();
        let export_items: Vec<TokenStream> = list.drain(..).map(|v| {
            TokenStream::from_str(&v)
        })
        .filter_map(|x| x.ok())
        .collect();

        // TODO: find a nice way to export the doc comment of the main export item...
        // #(#attrs)*
        // #[doc = #export_str]
        // #export

        if include_super {
            quote! {
                mod #module_name {
                    use super::LibraryObj;
                    use super::UserData;
                    #(#export_items)*
                }
            }
        } else {
            quote! {
                mod #module_name {
                    #(#export_items)*
                }
            }
        }
    }

    pub fn verify(&self, conf: &mut HiraConfig, loaded_from_path: &str) -> String {
        let mut out = String::new();
        if self.name.is_empty() {
            out = format!("Failed to find `const {HIRA_MOD_NAME_NAME}` from '{loaded_from_path}'\nMust provide a hira module name");
            return out;
        }
        let split = self.name.split("_");
        let num_components = split.into_iter().count();
        if num_components != 2 {
            out = format!("Invalid `const {HIRA_MOD_NAME_NAME} = \"{}\"` from '{loaded_from_path}'\nhira module name must contain 1 underscore.", self.name);
            return out;
        }
        if conf.loaded_modules.contains_key(&self.name) {
            out = format!("Duplicate module loading error from '{}'. Module '{}' already exists", loaded_from_path, self.name);
            return out;
        }
        for req in &self.required_crates {
            if !conf.known_cargo_dependencies.contains(req) {
                out = format!("hira module '{}' depends on crate '{}'. Add this to your Cargo.toml file.", self.name, req);
                return out;
            }
        }
        for req in &self.required_hira_modules {
            if !conf.loaded_modules.contains_key(req) {
                // if we haven't loaded this module yet, then go and try to load it.
                // TODO: eventually we want to convert req from
                // module_name -> module:name:
                // ie: make it the remote format.. we wish to only support remote formats for
                // requiring loaded modules. and then remote module lookups will first
                // try to look up the module from disk (ie: implied name).
                // however, for now we just try to look it up as if it's an implied name
                // so we keep the name as is:
                let module = match load_module(conf, req.clone()) {
                    Ok(m) => m,
                    Err(e) => {
                        out = e;
                        return out;
                    }
                };
                conf.loaded_modules.insert(module.name.clone(), module);
            }
        }
        if self.primary_export_item.is_empty() {
            out = format!("hira module '{}' is missing a primary export item. Expected to find `type {} = Something`", self.name, EXPORT_ITEM_NAME);
            return out;
        }
        if self.entrypoint_fn.is_none() {
            out = format!("hira module '{}' is missing an entrypoint function. Expected entrypoint function something like `pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut {}))`", self.name, self.primary_export_item);
            return out;
        }
        out
    }
}

fn set_entrypoint_fn(module: &mut HiraModule, item: &syn::ItemFn) -> bool {
    if item.sig.ident.to_string() != FN_ENTRYPOINT_NAME {
        return true
    }
    // enforce 2 args: the first is the LibraryObj
    // the 2nd is the callback to the user's function.
    // but too lazy to parse the callback signature right now. we just assume its valid..
    let input = if item.sig.inputs.len() != 2 {
        return true
    } else {
        item.sig.inputs.first().unwrap()
    };
    let input = match input {
        syn::FnArg::Typed(t) => t,
        _ => return true,
    };
    let reference = match *input.ty {
        syn::Type::Reference(ref r) => r.clone(),
        _ => return true,
    };
    if reference.mutability.is_none() {
        return true
    }
    let type_path = match *reference.elem {
        syn::Type::Path(p) => p,
        _ => return true,
    };
    let first = match type_path.path.segments.first() {
        Some(s) => s,
        None => return true,
    };
    if first.ident.to_string() != LIB_OBJ_TYPE_NAME {
        return true
    }
    // we verified this is the wasm_entrypoint fn, so set its signature to the module
    module.entrypoint_fn = Some(item.sig.to_token_stream().to_string());
    true
}

fn set_required_crates(module: &mut HiraModule, item: &syn::ItemConst) -> bool {
    if item.ident.to_string() != REQUIRED_CRATES_NAME {
        return true;
    }
    iterate_expr_for_strings(&*item.expr, |a| {
        module.required_crates.push(a);
    });
    true
}

fn set_module_name(module: &mut HiraModule, item: &syn::ItemConst) -> bool {
    if item.ident.to_string() != HIRA_MOD_NAME_NAME {
        return true;
    }
    if let syn::Expr::Lit(l) = &*item.expr {
        if let syn::Lit::Str(s) = &l.lit {
            let mut s = s.value();
            remove_surrounding_quotes(&mut s);
            module.name = s;
        }
    }
    true
}

fn fill_hira_mods(use_item: &syn::UseTree, hira_mods: &mut Vec<String>) -> String {
    let mut out = String::new();
    match &use_item {
        syn::UseTree::Name(n) => {
            let mut name = n.ident.to_string();
            remove_surrounding_quotes(&mut name);
            // require single underscore to be a hira module:
            let num_sections = name.split("_").into_iter().count();
            if num_sections == 2 {
                hira_mods.push(name);
            }
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                let out = fill_hira_mods(item, hira_mods);
                if !out.is_empty() {
                    return out;
                }
            }
        }
        x => {
            let x_str = x.to_token_stream().to_string().replace(" ", "");
            out = format!("Unsupported module use statement 'use {x_str}'.\nhira only supports use statements such as\n`use specific_module;`\nOr:\n`use {{\n  module_one,\n  module_two\n}}`");
            return out;
        }
    }
    out
}

fn set_required_hira_mods(module: &mut HiraModule, item: &syn::ItemUse) -> bool {
    let mut hira_mods = vec![];
    // during compilation of a whole file, we're ignoring the requirements somewhat
    // so we can ignore any errors while parsing this section.
    // any other errors due to invalid imports will be exposed during compilation.
    let _ = fill_hira_mods(&item.tree, &mut hira_mods);
    for m in hira_mods {
        module.required_hira_modules.push(m);
    }
    // we never want to let use:X statements
    // make it into the compilation, because these wont work.
    false
}

fn set_primary_export_item(module: &mut HiraModule, item: &syn::ItemType) -> bool {
    if item.ident.to_string() != EXPORT_ITEM_NAME {
        return true;
    }

    if let syn::Type::Path(ref p) = *item.ty {
        if let Some(seg) = p.path.segments.last() {
            if p.path.segments.len() == 1 {
                module.primary_export_item = seg.ident.to_string();
            }
        }
    }
    true
}

/// to add typehints while writing hira modules, writers
/// can add a #[hira::hira] macro above some const item.
/// we must remove these from the wasm code otherwise it'll try
/// to load some macro that doesn't exist at compile time for wasm modules.
fn remove_recursive_hira_macro(_module: &mut HiraModule, item: &syn::ItemMod) -> bool {
    for item in &item.attrs {
        let path = item.meta.path();
        let path_str = path.to_token_stream().to_string();
        if path_str.contains("hira") {
            return false;
        }
    }
    true
}

/// made this into a module because then i can collapse it while editing.
/// all of these are almost the same
mod set_exports {
    use super::*;

    pub fn set_export_item_macro(module: &mut HiraModule, item: &syn::ItemMacro) -> bool {
        let should_export = item.attrs.iter()
            .any(|x| x.meta.path().to_token_stream().to_string().contains("do_compile"));
        if should_export {
            let mut item = item.clone();
            item.attrs.clear();
            let mut name = item.ident.to_token_stream().to_string();
            remove_surrounding_quotes(&mut name);
            let mut out_string = item.to_token_stream().to_string();
            out_string = format!("#[macro_export]{out_string}");
            out_string.push_str(&format!("\npub(crate) use {name};"));
            module.export_items.insert(name, out_string);
        }
        true
    }
    pub fn set_export_item_impl(module: &mut HiraModule, item: &syn::ItemImpl) -> bool {
        let should_export = item.attrs.iter()
            .any(|x| x.meta.path().to_token_stream().to_string().contains("do_compile"));
        if should_export {
            let mut item = item.clone();
            item.attrs.clear();
            let s = item.to_token_stream().to_string();
            // TODO: why are module export items a hashmap?... just make it a list. this is silly
            module.export_items.insert(s.clone(), s);
        }
        true
    }
    pub fn set_export_item_enum(module: &mut HiraModule, item: &syn::ItemEnum) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_mod(module: &mut HiraModule, item: &syn::ItemMod) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_static(module: &mut HiraModule, item: &syn::ItemStatic) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_struct(module: &mut HiraModule, item: &syn::ItemStruct) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        !item.attrs.iter().any(|x| x.meta.path().to_token_stream().to_string().contains("dont_compile"))
    }
    pub fn set_export_item_trait(module: &mut HiraModule, item: &syn::ItemTrait) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_union(module: &mut HiraModule, item: &syn::ItemUnion) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_fn(module: &mut HiraModule, item: &syn::ItemFn) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            let fn_name = item.sig.ident.to_string();
            if fn_name == FN_ENTRYPOINT_NAME {
                return true;
            }
            module.export_items.insert(fn_name, item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_const(module: &mut HiraModule, item: &syn::ItemConst) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
    pub fn set_export_item_type(module: &mut HiraModule, item: &syn::ItemType) -> bool {
        if let syn::Visibility::Public(_) = item.vis {
            module.export_items.insert(item.ident.to_string(), item.to_token_stream().to_string());
        }
        true
    }
}

/// Note: this function doesn't know where the module was loaded from. it sets loaded_from to Implied
/// by default, but the caller of this function should override this value.
pub fn load_module_from_file_string(conf: &mut HiraConfig, path: &str, module_string: String) -> Result<HiraModule, String> {
    let mut out = HiraModule::default();
    
    let module_file = match parse_file(&module_string) {
        Ok(p) => p,
        Err(e) => {
            return Err(format!("Failed to parse '{}' as valid rust code.\nError:\n{:?}", path, e));
        }
    };

    let contents = iterate_file(
        &mut out, module_file,
        &[set_entrypoint_fn, set_exports::set_export_item_fn],
        &[set_required_crates, set_exports::set_export_item_const, set_module_name],
        &[set_primary_export_item, set_exports::set_export_item_type],
        &[set_exports::set_export_item_enum],
        &[set_exports::set_export_item_mod, remove_recursive_hira_macro],
        &[set_exports::set_export_item_macro],
        &[set_exports::set_export_item_static],
        &[set_exports::set_export_item_struct],
        &[set_exports::set_export_item_trait],
        &[set_exports::set_export_item_union],
        &[set_exports::set_export_item_impl],
        &[set_required_hira_mods],
    );
    out.contents = contents;

    let err_str = out.verify(conf, path);
    if !err_str.is_empty() {
        return Err(err_str);
    }

    Ok(out)
}


fn load_module_external_file(conf: &mut HiraConfig, path: String) -> Result<HiraModule, String> {
    let mut out = load_module_implied_file(conf, path)?;
    out.loaded_from = LoadedFrom::ExternalFile;
    Ok(out)
}

/// Path must be absolute prior to calling this. Even though it's implied. this just makes
/// the code more modular :P
fn load_module_implied_file(conf: &mut HiraConfig, path: String) -> Result<HiraModule, String> {
    let module_file = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!("Failed to read hira module from path '{}'\nError:\n{:?}", path, e));
        }
    };

    let mut out = load_module_from_file_string(conf, &path, module_file)?;
    out.loaded_from = LoadedFrom::Implied;
    Ok(out)
}

/// given some type of module string, load and return the module.
/// four types of modules depending on the string provided:
/// - external file. either a absolute/relative file path
/// - remote module
/// - implied module directory
/// we use the following methods to detect which type of module we will load
/// - absolute file path. we detect these if they start with /
/// - if its not the above, we cheeck if it is a remote module.
///   remote modules can come in 2 flavors:
///     - a namespace + modulename format like "mygithubusername:myrepository:"
///       we detect these by checking if there are exactly 2 colons, and the last
///       character must be a colon.
///     - an exact URL. we detect these if it starts with http:// or https://
/// - if not any of the above, then check if its a relative file path.
///   we detect these by checking if there is a / somewhere in the string.
/// - if its none of the above, we check if its a module name. these don't contain
///   any / at all, and can optionally end with the rust file extension .rs
///   if we find this, then we assume it's in the hira/modules directory.
pub fn load_module(conf: &mut HiraConfig, path: String) -> Result<HiraModule, String> {
    let is_absolute = path.starts_with("/");
    let is_namespace_modname_format = path.ends_with(":") && path.match_indices(":").collect::<Vec<_>>().len() == 2;
    let is_remote = path.starts_with("http://") || path.starts_with("https://") || is_namespace_modname_format;
    let is_relative = path.contains("/");
    let is_implied = !is_relative && !path.ends_with(":");

    if is_absolute || is_relative {
        let source_path = if is_relative {
            format!("{}/{}", conf.cargo_directory, path)
        } else {
            path.clone()
        };
        return load_module_external_file(conf, source_path);
    }

    if is_implied {
        let path_with_extension = if path.ends_with(".rs") {
            path
        } else {
            format!("{path}.rs")
        };
        let source_path = format!("{}/{}", conf.modules_directory, path_with_extension);
        return load_module_implied_file(conf, source_path);
    }

    if is_remote {
        unimplemented!("hira currently does not support remote modules");
    }

    return Err(
        format!("hira failed to load module. Unknown path format '{path}'")
    );
}


/// Corresponds to the hira_modules! macro entrypoint.
/// Given a macro token stream, read all the macro module paths, resolve the module
/// and save it to the hiraConfig, and output it.
pub fn load_modules(mut stream: TokenStream) -> TokenStream {
    let mut out = Err(default_stream());
    let out_ref = &mut out;
    use_hira_config(|conf| {
        let stream = std::mem::take(&mut stream);
        *out_ref = load_modules_inner(conf, stream);
    });
    match out {
        Ok(list) => {
            quote! {
                #(#list)*
            }
        }
        Err(e) => e,
    }
}

/// corresponds to the main hira! macro
pub fn run_module(mut stream: TokenStream, mut attr: TokenStream) -> TokenStream {
    let mut out = Err(default_stream());
    let out_ref = &mut out;
    use_hira_config(|conf| {
        let stream = std::mem::take(&mut stream);
        let attr = std::mem::take(&mut attr);
        *out_ref = run_module_inner(conf, stream, attr);
    });
    match out {
        Ok(o) => o,
        Err(e) => e,
    }
}

fn load_hira_dependencies_from_stream(stream: TokenStream) -> Result<Vec<String>, String> {
    let item_use = if let Ok(x) = syn::parse2::<ItemUse>(stream) {
        x
    } else {
        return Ok(vec![])
    };
    let mut hira_mods = vec![];
    let out = fill_hira_mods(&item_use.tree, &mut hira_mods);
    if out.is_empty() {
        Ok(hira_mods)
    } else {
        Err(out)
    }
}

/// this function corresponds to when a hira module writer wants type hints in their module.
/// they will add a line like `#[hira::hira] use { dependencies ... }`
/// to insert the library object data + dependencies into their code.
pub fn run_module_include_only(conf: &mut HiraConfig, stream: TokenStream) -> Result<TokenStream, TokenStream> {
    // try to parse the item stream as a use statement
    let required_mods = match load_hira_dependencies_from_stream(stream) {
        Ok(r) => r,
        Err(e) => {
            return Err(compiler_error(&e));
        }
    };

    let mut extra_mod_defs = vec![];
    for required_mod in required_mods {
        // TODO: currently the mod name "path" is in implied name format
        // ie: module_name, so the behavior will be to only search for it in
        // the modules/ directory. should change this to be a remote name format
        // such that load_modules will try to look for it either
        // online or in the modules/ directory
        let module = match load_module(conf, required_mod) {
            Ok(o) => o,
            Err(e) => {
                return Err(compiler_error(&e));
            }
        };
        extra_mod_defs.push(module.to_token_stream(true));
    }

    let mut include_str = LibraryObj::include_in_rs_wasm();
    include_str.push_str(user_data_impl());
    include_str.push_str(lib_obj_impl());
    let include_tokens = proc_macro2::TokenStream::from_str(&include_str).unwrap_or_default();
    let parsing_tokens = proc_macro2::TokenStream::from_str(WASM_PARSING_TRAIT_STR).unwrap_or_default();
    let out = quote! {
        #parsing_tokens

        #(#extra_mod_defs)*

        #include_tokens
    };
    return Ok(TokenStream::from(out));
}

pub fn run_module_validate_user_input(
    stream: TokenStream, attr: &TokenStream
) -> Result<(InputType, String, u32), TokenStream> {
    let item_str = stream.to_string();
    let attr_str = attr.to_string();
    let combined = format!("{item_str}{attr_str}");
    let hash = adler32::adler32(combined.as_bytes()).unwrap_or(0);

    let input_type = get_input_type(stream);
    // verify the input is something that we support. currently:
    // - entire functions, signature + body.
    // - derive input, ie: struct defs, enums.
    let input_type = if let Some(input) = input_type {
        input
    } else {
        return Err(compiler_error("hira was applied to an item that we currently do not support parsing. Currently only supports functions and deriveInputs"));
    };

    let depends_on_module = match parse_callback_required_module(attr_str) {
        Ok(m) => m,
        Err(e) => return Err(compiler_error(&e)),
    };

    Ok((input_type, depends_on_module, hash))
}

pub fn load_module_default(mut items: TokenStream) -> TokenStream {
    let mut out = Err(default_stream());
    let out_ref = &mut out;
    use_hira_config(|conf| {
        let items = std::mem::take(&mut items);
        *out_ref = load_module_default_inner(conf, items);
    });
    match out {
        Ok(o) => o,
        Err(e) => e,
    }
}

pub fn load_module_default_inner(conf: &mut HiraConfig, items: TokenStream) -> Result<TokenStream, TokenStream> {
    let (path, rest_of_items) = extract_default_attr(items)?;
    let module = match load_module(conf, path) {
        Ok(o) => o,
        Err(e) => {
            return Err(compiler_error(&e));
        }
    };

    let fn_ident = format_ident!("_{}_default", module.name);
    let mod_def = module.to_token_stream(false);
    conf.default_callbacks.insert(module.name.clone(), rest_of_items.to_string());
    conf.loaded_modules.insert(module.name.clone(), module);

    Ok(quote! {
        #mod_def

        fn #fn_ident() {
            let cb = #rest_of_items;
        }
    })
}

pub fn run_module_inner(conf: &mut HiraConfig, stream: TokenStream, mut attr: TokenStream) -> Result<TokenStream, TokenStream> {
    // this is a hack to allow people who write wasm_modules easy type hints.
    // if we detect no attributes, then we just output all of the types that
    // wasm module writers depend on, like UserData, and LibraryObj
    if attr.is_empty() {
        return run_module_include_only(conf, stream);
    }
    // otherwise, it's a normal module macro, ie a `#[hira(|callback| { ... })]`
    // so:
    // 1. validate the user's input:
    //    a. ensure the required module in the callback exists.
    //    b. ensure the user's input stream is valid (ie: function, struct, module, etc.)
    //    c. extract/create necessary hashes/item identifiers for following steps
    // 2. output the users callback so they get typehints
    // 3. run+compile the wasm
    // 4. change the outputs according to the wasm library object result

    let (mut input_type, module_name, hash) = run_module_validate_user_input(stream.clone(), &attr)?;

    // need to get the module once and clone its required crates
    // and then get the module again after loading all of its requirements...
    // should be a fast operation once the modules are loaded though
    let mut requirements = {
        let module = conf.get_module(&module_name).map_err(|e| compiler_error(&e))?;
        module.required_hira_modules.clone()
    };
    requirements.sort();
    let mut extra_mod_defs = vec![];
    for req in requirements {
        let req_module = conf.get_module(&req).map_err(|e| compiler_error(&format!("Failed to load required module for '{}'\n{:?}", module_name, e)))?;
        extra_mod_defs.push(req_module.to_token_stream(true));
    }
    let default_cb = conf.default_callbacks.get(&module_name).map(|x| x.to_owned());
    let hira_base_code = conf.hira_base_code.clone();
    let module = conf.get_module(&module_name).map_err(|e| compiler_error(&e))?;

    // form the code that we will actually compile:
    let parsed_wasm_code = parse_file(&module.contents).map_err(|e| {
        compiler_error(&format!("Failed to parse '{}' as valid rust code. Error:\n{:?}", module.name, e))
    })?;
    let item_name = input_type.get_name();
    let code = get_wasm_code_to_compile(
        hira_base_code, &module_name, &item_name,
        &module.primary_export_item, &attr, parsed_wasm_code,
        extra_mod_defs, default_cb
    );

    let mut pass_this = LibraryObj::new();
    // pass_this.user_data = (&input_type).into();
    // pass_this.dependencies = Vec::from_iter(conf.known_cargo_dependencies.clone());
    // pass_this.shared_state = conf.shared_data.clone();
    // pass_this.crate_name = std::env::var("CARGO_CRATE_NAME").unwrap_or("".into());
    let mut lib_obj = get_wasm_output(
        &conf.wasm_directory,
        &code,
        &pass_this
    ).unwrap_or_default();

    // if !lib_obj.compiler_error_message.is_empty() {
    //     // TODO: currently we just add a compile_error to the end of the stream..
    //     // in the future maybe search for a string, and replace the right hand side to compile_error
    //     // so that we can put it on a specific line
    //     let err = compiler_error(&lib_obj.compiler_error_message);
    //     attr.extend([err]);
    // }

    // let mut add_after = vec![];
    // for s in lib_obj.add_code_after.drain(..) {
    //     let tokens = TokenStream::from_str(&s).map_err(|e| {
    //         compiler_error(&format!("Module '{}' produced invalid after_code tokens:\n{}\nError:\n{:?}", module_name, s, e))
    //     })?;
    //     add_after.push(tokens);
    // }

    conf.do_file_ops(&module_name, &mut lib_obj).map_err(|e| {
        compiler_error(&e)
    })?;
    // conf.save_shared_data(std::mem::take(&mut lib_obj.shared_state));
    input_type.apply_library_obj_changes(lib_obj, &module_name);
    let item = input_type.back_to_stream(&format!("_b{hash}"));

    let func_name = format_ident!("_a{hash}");
    let user_out = quote! {
        // we use a random hash for the func name to not conflict with other invocations of this macro
        fn #func_name() {
            let cb = #attr;
        }
        #item

        // #(#add_after)*
    };

    Ok(user_out)
}

pub fn load_modules_inner(conf: &mut HiraConfig, stream: TokenStream) -> Result<Vec<TokenStream>, TokenStream> {
    let module_strings = get_list_of_strings(stream);
    let mut out = vec![];

    let module_dir = &conf.modules_directory;
    let _ = std::fs::create_dir_all(module_dir);

    for path in module_strings {
        let module = match load_module(conf, path) {
            Ok(o) => o,
            Err(e) => {
                return Err(compiler_error(&e));
            }
        };
        out.push(module.to_token_stream(false));
        conf.loaded_modules.insert(module.name.clone(), module);
    }
    Ok(out)
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

pub fn hira_mod2_inner(conf: &mut HiraConfig, stream: TokenStream) -> Result<TokenStream, TokenStream> {
    let mut module = parse_module_from_stream(stream.clone())?;
    module.verify_config_signature()?;

    // only level3 modules get compiled into wasm
    // all other modules get compiled as dependencies for a level3 module
    // but theres no point to compile them all individually
    if module.level != ModuleLevel::Level3 {
        conf.modules2.insert(module.name.clone(), module);
        return Ok(stream);
    }

    let codes = get_wasm_code_to_compile2(conf, &module)?;

    let mut pass_this = LibraryObj::new();
    // TODO: fill in library obj according to the required capabilities
    let mut lib_obj = get_wasm_output(
        &conf.wasm_directory,
        &codes,
        &pass_this
    ).unwrap_or_default();

    lib_obj.apply_changes(conf, &mut module)?;
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
    );
    Ok(hira_mod)
}


#[cfg(test)]
mod tests {
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
        let out = module.verify_config_signature();
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
        let out = module.verify_config_signature();
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
        let out = module.verify_config_signature();
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
        let out = module.verify_config_signature();
        assert!(out.is_err());
        let err = out.err().unwrap().to_string();
        assert!(err.contains("Expected to only see use statements from Level2 module l2_dep, but found some_other_dep"));
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

    #[test]
    fn can_remove_recursive_macro() {
        let code = r#"
        #[hira::hira] mod _typehints {}
        const HIRA_MODULE_NAME: &'static str = "a_b";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string()).unwrap();
        assert_eq!(res.contents, "const HIRA_MODULE_NAME : & 'static str = \"a_b\" ;\ntype ExportType = Something ;\npub fn wasm_entrypoint (obj : & mut LibraryObj , cb : fn (& mut Something)) { }\n");
    }

    #[test]
    fn fails_if_module_name_not_provided() {
        let code = r#"
        const HIRA_MODULE_NAME_WRONG: &'static str = "aaa";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert_eq!(err, "Failed to find `const HIRA_MODULE_NAME` from 'a'\nMust provide a hira module name");
    }

    #[test]
    fn can_load_module_name() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        assert!(res.is_ok());
        let module = res.ok().unwrap();
        assert_eq!(module.name, "hello_world");
    }

    #[test]
    fn cant_have_duplicate_modules() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        conf.loaded_modules.insert("hello_world".to_string(), Default::default());
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert_eq!(err, "Duplicate module loading error from 'a'. Module 'hello_world' already exists");
    }

    #[test]
    fn hira_can_warn_user_of_missing_crate() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        const REQUIRED_CRATES: &[&'static str] = &["tokio"];
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        let err = res.err().unwrap();
        assert_eq!(err, "hira module 'hello_world' depends on crate 'tokio'. Add this to your Cargo.toml file.");
    }

    #[test]
    fn hira_can_pass_macros_to_other_modules() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}

        #[hira::do_compile]
        macro_rules! hello {
            ($dest:ident) => {
                stringify!($dest)
            };
        }
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        let module = res.ok().unwrap();
        let included_code = module.to_token_stream(false).to_string();
        assert_eq!(included_code, "mod hello_world { # [macro_export] macro_rules ! hello { ($ dest : ident) => { stringify ! ($ dest) } ; } pub (crate) use hello ; }");
    }

    #[test]
    fn hira_can_pass_impls_to_other_modules() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}

        pub struct Hello {}
        #[hira::do_compile]
        impl Hello {
            pub fn say_hi() { println!("hi"); }
        }
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        let module = res.ok().unwrap();
        let included_code = module.to_token_stream(false).to_string();
        assert!(included_code.contains("impl Hello"));
    }

    #[test]
    fn hira_can_store_required_modules() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        const REQUIRED_CRATES: &[&'static str] = &["tokio"];
        type ExportType = Something;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut Something)) {}
        "#;
        let mut conf = HiraConfig::default();
        conf.known_cargo_dependencies.insert("tokio".to_string());
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        let module = res.ok().unwrap();
        assert!(module.required_crates.contains(&"tokio".to_string()));
    }

    #[test]
    fn hira_doesnt_export_wasm_entrypoint() {
        let code = r#"
        const HIRA_MODULE_NAME: &'static str = "hello_world";
        type ExportType = CloudfrontInput;
        pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut CloudfrontInput)) -> CloudfrontInput {}
        "#;
        let mut conf = HiraConfig::default();
        let res = load_module_from_file_string(&mut conf, "a", code.to_string());
        let module = res.ok().unwrap();
        assert!(module.entrypoint_fn.is_some());
        assert!(!module.export_items.contains_key("wasm_entrypoint"));
    }
}
