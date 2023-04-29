//! module w/ helper functions related to parsing from
//! token streams into hira specific structures
//! 

use std::str::FromStr;

use proc_macro2::{
    TokenStream,
    TokenTree, Ident,
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
    Expr, ItemUse, Visibility, token::Pub
};

use crate::{module_loading::HiraModule, wasm_types::{InputType, GlobalVariable}};

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
    use_callbacks: &[fn(&mut HiraModule, &ItemUse) -> bool],
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
            Item::Use(x) => {
                for cb in use_callbacks {
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

pub fn rename_ident(id: &mut Ident, name: &str) {
    if id.to_string() != name {
        let span = id.span();
        let new_ident = Ident::new(name, span);
        *id = new_ident;
    }
}

pub fn is_public(vis: &Visibility) -> bool {
    match vis {
        Visibility::Public(_) => true,
        _ => false,
    }
}

pub fn set_visibility(vis: &mut Visibility, is_pub: bool) {
    let p = Pub::default();
    match (&vis, is_pub) {
        (Visibility::Public(_), false) => {
            *vis = Visibility::Inherited;
        }
        (Visibility::Restricted(_), true) => {
            *vis = Visibility::Public(p);
        }
        (Visibility::Inherited, true) => {
            *vis = Visibility::Public(p);
        }
        _ => {}
    }
}

pub fn get_input_type(item: proc_macro2::TokenStream) -> Option<InputType> {
    let is_struct_input = syn::parse2::<ItemStruct>(item.clone()).ok();
    if let Some(struct_input) = is_struct_input {
        return Some(InputType::Struct(struct_input));
    }
    let is_fn_input = syn::parse2::<ItemFn>(item.clone()).ok();
    if let Some(function_input) = is_fn_input {
        return Some(InputType::Function(function_input));
    }
    let is_static_input = syn::parse2::<ItemStatic>(item.clone()).ok();
    if let Some(input) = is_static_input {
        return Some(InputType::GlobalVar(GlobalVariable::Static(input)));
    }
    let is_const_input = syn::parse2::<ItemConst>(item.clone()).ok();
    if let Some(input) = is_const_input {
        if input.ident.to_string() == "_" {
            if let syn::Expr::Match(m) = *input.expr {
                return Some(InputType::Match(m));
            }
        }
        return Some(InputType::GlobalVar(GlobalVariable::Constant(input)));
    }
    let is_mod_input = syn::parse2::<ItemMod>(item.clone()).ok();
    if let Some(input) = is_mod_input {
        return Some(InputType::Module(input));
    }
    None
}

/// given the user's attribute in the hira macro, parse out the name of the module they are referencing
pub fn parse_callback_required_module(attr_str: String) -> Result<String, String> {
    let err_str = "Failed to parse signature of macro attribute. Expected a closure like |obj: &mut modulename::StructName| {{ ... }}";
    // get everything in callback input signature |mything: &mut modulename::StructName| { ... }
    let splits: Vec<_> = attr_str.split("|").collect();
    let signature = match splits.get(1) {
        Some(s) => *s,
        None => return Err(format!("{}", err_str)),
    };
    // now signature looks like
    // mything: &mut modulename::StructName
    // actually it has spaces around it, but we can solve that by just removing the spaces
    let signature_nospace = signature.replace(" ", "");
    let after_mut = if let Some((_, b)) = signature_nospace.split_once("&mut") {
        b.trim()
    } else {
        return Err(format!("{}", err_str));
    };

    if let Some((mod_name, _)) = after_mut.split_once("::") {
        Ok(mod_name.to_string())
    } else {
        Err(format!("{}", err_str))
    }
}

pub fn extract_default_attr(stream: TokenStream) -> Result<(String, TokenStream), TokenStream> {
    let mut items_iter = stream.into_iter();
    let path = match items_iter.next() {
        Some(proc_macro2::TokenTree::Literal(l)) => {
            let mut path = l.to_string();
            remove_surrounding_quotes(&mut path);
            path
        }
        _ => return Err(compiler_error("hira_module_default expects first argument to be a literal string of your module path")),
    };

    let mut has_punct = false;
    if let Some(proc_macro2::TokenTree::Punct(p)) = items_iter.next() {
        if p.as_char() == ',' {
            has_punct = true;
        }
    }
    if !has_punct {
        return Err(compiler_error("hira_module_default expects a comma after module path and before your callback"));
    }
    // we assume that the user entered this correctly. we dont do any validation to speed up compilation.
    // if the user makes an error here, it should be showed to them by their IDE/compiler.
    let rest_of_items: proc_macro2::TokenStream = items_iter.collect();
    Ok((path, rest_of_items))
}
