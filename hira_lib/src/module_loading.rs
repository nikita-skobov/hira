use std::collections::HashMap;
use std::str::FromStr;

use proc_macro2::TokenStream;
use quote::{quote, format_ident, ToTokens};
use syn::{parse_file, ItemUse};

use crate::parsing::{remove_surrounding_quotes, get_input_type, parse_callback_required_module};
use crate::wasm_types::*;
use wasm_type_gen::*;

use super::HiraConfig;
use super::parsing::{default_stream, compiler_error, iterate_file, iterate_expr_for_strings, get_list_of_strings};
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
    pub fn to_token_stream(&self) -> TokenStream {
        let module_name = format_ident!("{}", self.name);
        let export_items: Vec<TokenStream> = self.export_items.iter().map(|(_, v)| {
            TokenStream::from_str(v)
        })
        .filter_map(|x| x.ok())
        .collect();

        // TODO: find a nice way to export the doc comment of the main export item...
        // #(#attrs)*
        // #[doc = #export_str]
        // #export
        let stream = quote! {
            mod #module_name {
                #(#export_items)*
            }
        };
        stream
    }
    pub fn verify(&self, conf: &mut HiraConfig) -> String {
        let mut out = String::new();
        if self.name.is_empty() {
            out = format!("Failed to find `const {HIRA_MOD_NAME_NAME}`\nMust provide a hira module name");
            return out;
        }
        let split = self.name.split("_");
        let num_components = split.into_iter().count();
        if num_components != 2 {
            out = format!("Invalid `const {HIRA_MOD_NAME_NAME} = \"{}\"`\nhira module name must contain 1 underscore.", self.name);
            return out;
        }
        if conf.loaded_modules.contains_key(&self.name) {
            out = format!("Duplicate module loading error. '{}' already exists", self.name);
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
        true
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
        &[set_exports::set_export_item_static],
        &[set_exports::set_export_item_struct],
        &[set_exports::set_export_item_trait],
        &[set_exports::set_export_item_union],
        &[set_required_hira_mods],
    );
    out.contents = contents;

    let err_str = out.verify(conf);
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
        extra_mod_defs.push(module.to_token_stream());
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
    let requirements = {
        let module = conf.get_module(&module_name).map_err(|e| compiler_error(&e))?;
        module.required_crates.clone()
    };
    let mut extra_mod_defs = vec![];
    for req in requirements {
        let req_module = conf.get_module(&req).map_err(|e| compiler_error(&format!("Failed to load required module for '{}'\n{:?}", module_name, e)))?;
        extra_mod_defs.push(req_module.to_token_stream());
    }
    let module = conf.get_module(&module_name).map_err(|e| compiler_error(&e))?;

    // form the code that we will actually compile:
    let parsed_wasm_code = parse_file(&module.contents).map_err(|e| {
        compiler_error(&format!("Failed to parse '{}' as valid rust code. Error:\n{:?}", module.name, e))
    })?;
    let (code, add_to_code) = get_wasm_code_to_compile(
        &module_name, &module.primary_export_item, &attr, parsed_wasm_code, extra_mod_defs
    );

    let item_name = input_type.get_name();
    let mut pass_this = LibraryObj::new();
    pass_this.user_data = (&input_type).into();
    pass_this.dependencies = Vec::from_iter(conf.known_cargo_dependencies.clone());
    pass_this.shared_state = conf.shared_data.clone();
    pass_this.crate_name = std::env::var("CARGO_CRATE_NAME").unwrap_or("".into());
    let mut lib_obj = get_wasm_output(
        Some(conf.wasm_directory.clone()),
        &item_name,
        code,
        add_to_code, 
        &pass_this
    ).unwrap_or_default();

    if !lib_obj.compiler_error_message.is_empty() {
        // TODO: currently we just add a compile_error to the end of the stream..
        // in the future maybe search for a string, and replace the right hand side to compile_error
        // so that we can put it on a specific line
        let err = compiler_error(&lib_obj.compiler_error_message);
        attr.extend([err]);
    }

    let mut add_after = vec![];
    for s in lib_obj.add_code_after.drain(..) {
        let tokens = TokenStream::from_str(&s).map_err(|e| {
            compiler_error(&format!("Module '{}' produced invalid after_code tokens:\n{}\nError:\n{:?}", module_name, s, e))
        })?;
        add_after.push(tokens);
    }

    // TODO:
    // save_shared_mem_data(std::mem::take(&mut lib_obj.shared_state));
    // if should_output_command_files {
    //     if let Err(e) = lib_obj.handle_file_ops(module_name, &item_name) {
    //         panic!("{}", e);
    //     }
    // }

    conf.save_shared_data(std::mem::take(&mut lib_obj.shared_state));
    input_type.apply_library_obj_changes(lib_obj, &module_name);
    let item = input_type.back_to_stream(&format!("_b{hash}"));

    let func_name = format_ident!("_a{hash}");
    let user_out = quote! {
        // we use a random hash for the func name to not conflict with other invocations of this macro
        fn #func_name() {
            let cb = #attr;
        }
        #item

        #(#add_after)*
    };

    Ok(user_out)
}

pub fn load_modules_inner(conf: &mut HiraConfig, stream: TokenStream) -> Result<Vec<TokenStream>, TokenStream> {
    let module_strings = get_list_of_strings(stream);
    let mut out = vec![];
    for path in module_strings {
        let module = match load_module(conf, path) {
            Ok(o) => o,
            Err(e) => {
                return Err(compiler_error(&e));
            }
        };
        out.push(module.to_token_stream());
        conf.loaded_modules.insert(module.name.clone(), module);
    }
    Ok(out)
}

pub fn do_something_with_module(_stream: TokenStream) -> TokenStream {
    let mut out = default_stream();
    let out_ref = &mut out;
    use_hira_config(|conf| {
        let thing = &conf.loaded_modules["todo"];
        let text = &thing.contents;
        *out_ref = quote! {
            pub const something: &'static str = #text;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(err, "Failed to find `const HIRA_MODULE_NAME`\nMust provide a hira module name");
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
        assert_eq!(err, "Duplicate module loading error. 'hello_world' already exists");
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
