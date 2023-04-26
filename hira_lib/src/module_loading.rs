use proc_macro2::TokenStream;
use quote::{quote, format_ident, ToTokens};
use syn::{parse_file, File};

use crate::HiraConfig;
use crate::parsing::{default_stream, compiler_error};

use super::parsing::get_list_of_strings;
use super::use_hira_config;

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
}

impl Default for HiraModule {
    fn default() -> Self {
        Self {
            name: Default::default(),
            loaded_from: LoadedFrom::Implied,
            contents: Default::default(),
        }
    }
}

impl HiraModule {
    pub fn to_token_stream(&self) -> TokenStream {
        let module_name = format_ident!("{}", self.name);
        let stream = quote! {
            mod #module_name {
                // #(#attrs)*
                // #[doc = #export_str]
                // #export
            }
        };
        stream
    }
}

fn load_module_external_file(conf: &mut HiraConfig, path: String) -> Result<HiraModule, String> {
    let mut out = load_module_implied_file(conf, path)?;
    out.loaded_from = LoadedFrom::ExternalFile;
    Ok(out)
}

/// Note: this function doesn't know where the module was loaded from. it sets loaded_from to Implied
/// by default, but the caller of this function should override this value.
fn load_module_from_file_string(conf: &mut HiraConfig, module_file: File) -> Result<HiraModule, String> {
    Ok(HiraModule {
        name: "todo".to_string(),
        contents: module_file.to_token_stream().to_string(),
        loaded_from: LoadedFrom::Implied,
    })
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
    let parsed_wasm_code = match parse_file(&module_file) {
        Ok(p) => p,
        Err(e) => {
            return Err(format!("Failed to parse '{}' as valid rust code.\nError:\n{:?}", path, e));
        }
    };
    let mut out = load_module_from_file_string(conf, parsed_wasm_code)?;
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
fn load_module(conf: &mut HiraConfig, path: String) -> Result<HiraModule, String> {
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
