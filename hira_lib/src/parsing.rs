//! module w/ helper functions related to parsing from
//! token streams into hira specific structures
//! 

use std::str::FromStr;

use proc_macro2::{
    TokenStream,
    TokenTree,
};
use quote::ToTokens;
use syn::{
    File,
    Item,
    ItemFn,
    ItemConst,
    ItemType,
    ItemEnum,
    ItemMod,
    ItemStatic,
    ItemStruct,
    ItemTrait,
    ItemUnion,
    Expr
};

use crate::module_loading::HiraModule;

pub fn default_stream() -> TokenStream {
    compiler_error("Failed to get hira config")
}

pub fn compiler_error(msg: &str) -> TokenStream {
    let tokens = format!("compile_error!(r#\"{msg}\"#);");
    let out = match TokenStream::from_str(&tokens) {
        Ok(o) => o,
        Err(e) => {
            panic!("Failed to parse compiler_error formatting\n{:?}", e);
        }
    };
    out
}

/// in a few places in hira we let the module writer specify some array of values
/// which we parse out the strings. This function is generic over that iteration
/// and calls the callback with anytime we find a string
pub fn iterate_expr_for_strings(
    expr: &Expr,
    mut cb: impl FnMut(String)
) {
    let arr = match expr {
        syn::Expr::Array(arr) => arr,
        syn::Expr::Reference(r) => {
            if let syn::Expr::Array(arr) = &*r.expr {
                arr
            } else {
                return;
            }
        }
        _ => {
            return;
        }
    };
    for item in arr.elems.iter() {
        if let syn::Expr::Lit(l) = item {
            if let syn::Lit::Str(s) = &l.lit {
                let mut s = s.token().to_string();
                remove_surrounding_quotes(&mut s);
                cb(s);
            }
        }
    }
}

/// iterates over a file, calls provided callbacks which can modify the
/// hira module definition for the relevant fields.
/// each callback returns whether or not the item should be added to the exported string.
/// if any callback returns false, that particular item is ignored.
/// finally, return the output string which is the filtered
/// version of the file.
pub fn iterate_file(
    module: &mut HiraModule,
    file: File,
    fn_callbacks: &[fn(&mut HiraModule, &ItemFn) -> bool],
    const_callbacks: &[fn(&mut HiraModule, &ItemConst) -> bool],
    type_callbacks: &[fn(&mut HiraModule, &ItemType) -> bool],
    enum_callbacks: &[fn(&mut HiraModule, &ItemEnum) -> bool],
    mod_callbacks: &[fn(&mut HiraModule, &ItemMod) -> bool],
    static_callbacks: &[fn(&mut HiraModule, &ItemStatic) -> bool],
    struct_callbacks: &[fn(&mut HiraModule, &ItemStruct) -> bool],
    trait_callbacks: &[fn(&mut HiraModule, &ItemTrait) -> bool],
    union_callbacks: &[fn(&mut HiraModule, &ItemUnion) -> bool],
) -> String {
    let mut out = "".to_string();
    for item in file.items.iter() {
        let mut is_filtered = false;
        match item {
            Item::Fn(x) => {
                for cb in fn_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Const(x) => {
                for cb in const_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Enum(x) => {
                for cb in enum_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Mod(x) => {
                for cb in mod_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Static(x) => {
                for cb in static_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Struct(x) => {
                for cb in struct_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Trait(x) => {
                for cb in trait_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Type(x) => {
                for cb in type_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            Item::Union(x) => {
                for cb in union_callbacks {
                    if !cb(module, x) {
                        is_filtered = true;
                    }
                }
            }
            // ignore all other item types
            _ => {}
        }
        if !is_filtered {
            let item_str = item.to_token_stream().to_string();
            out.push_str(&item_str);
            out.push('\n');
        }
    }
    out
}

pub fn remove_surrounding_quotes(s: &mut String) {
    while s.starts_with('"') && s.ends_with('"') && s.len() > 1 {
        s.remove(0);
        s.pop();
    }
}

/// given an arbitrary token stream, iterate and find all string literals
/// and output a vector of the found strings. This method consumes the stream,
/// and ignores everything that is not a string literal. it does not recurse into Groups.
pub fn get_list_of_strings(stream: TokenStream) -> Vec<String> {
    let mut out = vec![];
    for item in stream {
        if let TokenTree::Literal(l) = item {
            let mut s = l.to_string();
            remove_surrounding_quotes(&mut s);
            out.push(s);
        }
    }
    out
}
