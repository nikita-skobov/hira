//! module w/ helper functions related to parsing from
//! token streams into hira specific structures
//! 

use std::str::FromStr;

use serde::{Serialize, Deserialize};

use proc_macro2::{
    TokenStream,
    TokenTree, Ident, Span,
};
use quote::{quote, ToTokens, format_ident};
use syn::{
    Item,
    ItemFn,
    ItemConst,
    ItemMod,
    ItemStruct,
    Expr, ItemUse, Visibility, token::Pub, ItemExternCrate, Meta, ItemImpl, Attribute, Fields
};

use crate::{module_loading::{HiraModule2, ModuleLevel, parse_module_from_stream}, HiraConfig};

#[cfg(feature = "wasm")]
use wasm_type_gen::*;

#[cfg_attr(feature = "wasm", derive(WasmTypeGen, Debug))]
#[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
pub struct UserInput {
    /// only relevant for input params to a function. not applicable to struct fields.
    pub is_self: bool,
    pub name: String,
    pub ty: String,
}

#[cfg_attr(feature = "wasm", derive(WasmTypeGen, Debug))]
#[derive(Default)]
#[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionSignature {
    pub name: String,
    pub is_pub: bool,
    pub is_async: bool,
    pub is_unsafe: bool,
    pub is_const: bool,
    pub inputs: Vec<UserInput>,
    pub return_ty: String,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Hiracfg {
    pub key: String,
    pub value: Option<String>,
    pub applied_to: String,
}

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

pub fn iterate_tuples(expr: &Expr, cb: &mut impl FnMut(String, &Expr)) {
    match expr {
        Expr::Array(array) => {
            for elem in array.elems.iter() {
                iterate_tuples(elem, cb);
            }
        }
        Expr::Reference(r) => {
            iterate_tuples(&*r.expr, cb);
        }
        Expr::Tuple(tuple) if tuple.elems.len() == 2 => {
            if let syn::Expr::Lit(l) = &tuple.elems[0] {
                let mut s1 = l.to_token_stream().to_string();
                remove_surrounding_quotes(&mut s1);
                cb(s1, &tuple.elems[1]);
            }
        }
        _ => (),
    }
}

pub fn parse_fn_signature(item: &ItemFn) -> FunctionSignature {
    let mut name = item.sig.ident.to_string();
    remove_surrounding_quotes(&mut name);
    let mut inputs = vec![];
    for input in item.sig.inputs.iter() {
        let usr_field = UserInput {
            is_self: match input {
                syn::FnArg::Receiver(_) => true,
                syn::FnArg::Typed(_) => false,
            },
            name: match input {
                syn::FnArg::Receiver(_) => "&self".into(),
                syn::FnArg::Typed(ty) => ty.pat.to_token_stream().to_string(),
            },
            ty: match input {
                syn::FnArg::Receiver(_) => "".into(),
                syn::FnArg::Typed(ty) => ty.ty.to_token_stream().to_string(),
            }
        };
        inputs.push(usr_field);
    }
    let return_ty = match &item.sig.output {
        syn::ReturnType::Default => "".into(),
        syn::ReturnType::Type(_, b) => b.to_token_stream().to_string(),
    };

    FunctionSignature {
        name,
        is_pub: match item.vis {
            Visibility::Public(_) => true,
            _ => false,
        },
        is_async: item.sig.asyncness.is_some(),
        is_unsafe: item.sig.unsafety.is_some(),
        is_const: item.sig.constness.is_some(),
        inputs,
        return_ty,
    }
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

/// given a list of paths of names into an item tree
/// such as "use A::B::C::outputs::something"
/// return a tuple of the module name (this is always 1 before the outputs)
/// and optionally if there is a field after outputs, the specific import
pub fn parse_module_name_from_use_tree(names: &[String]) -> Option<(&String, Option<&String>)> {
    let output_index = names.iter().position(|x| x == "outputs")?;
    let mod_name_index = if output_index > 0 { output_index - 1 } else { return None };
    let mod_name = &names[mod_name_index];
    if output_index == names.len() - 1 {
        return Some((mod_name, None));
    }
    // otherwise, there's something after the outputs
    let last = names.last()?;
    Some((mod_name, Some(last)))
}

/// given a path like `self::some_module::outputs`
/// return the name of the first real module name (excluding self, crate, super)
/// and a slice of all the names afterwards
pub fn parse_module_name_from_use_names(names: &[String]) -> Option<(&String, &[String])> {
    for (i, name) in names.iter().enumerate() {
        if name != "self" && name != "crate" && name != "super" {
            let next_index = i + 1;
            let slice = if next_index >= names.len() {
                &[]
            } else {
                &names[next_index..]
            };
            return Some((name, slice));
        }
    }
    None
}

/// callback takes 3 args:
/// - list of all paths into the use tree in order left to right.
/// - option of if this is a renamed item
/// - boolean if the last component is a wildcard.
pub fn iterate_item_tree(past_names: &mut Vec<String>, tree: &syn::UseTree, cb: &mut impl FnMut(&[String], Option<String>, bool)) {
    match tree {
        syn::UseTree::Name(n) => {
            let name = get_ident_string(&n.ident);
            past_names.push(name);
            cb(&past_names, None, false);
            past_names.pop();
        }
        syn::UseTree::Rename(n) => {
            let name = get_ident_string(&n.ident);
            let rename = get_ident_string(&n.rename);
            past_names.push(name);
            cb(&past_names, Some(rename), false);
            past_names.pop();
        }
        syn::UseTree::Path(p) => {
            let name = get_ident_string(&p.ident);
            let len = past_names.len();
            past_names.push(name);
            iterate_item_tree(past_names, &p.tree, cb);
            past_names.truncate(len);
        }
        syn::UseTree::Group(g) => {
            for item in &g.items {
                let len = past_names.len();
                iterate_item_tree(past_names, item, cb);
                past_names.truncate(len);
            }
        }
        syn::UseTree::Glob(_) => {
            cb(&past_names, None, true);
        }
    }
}

pub fn iterate_mod_def_generic<T>(
    thing: &mut T,
    mod_def: &mut ItemMod,
    fn_callbacks: &[fn(&mut T, &mut ItemFn)],
    struct_callbacks: &[fn(&mut T, &mut ItemStruct)],
    use_callbacks: &[fn(&mut T, &mut ItemUse)],
    mod_callbacks: &[fn(&mut T, &mut ItemMod)],
    const_callbacks: &[fn(&mut T, &mut ItemConst)],
    extern_crate_callbacks: &[fn(&mut T, &mut ItemExternCrate)],
    impl_callbacks: &[fn(&mut T, &mut ItemImpl)],
    fallback_cb: &[fn(&mut T, &mut Item)],
) {
    let mut default_vec = vec![];
    let content = mod_def.content.as_mut().map(|x| &mut x.1).unwrap_or(&mut default_vec);
    for item in content {
        match item {
            Item::Fn(x) => {
                for cb in fn_callbacks {
                    cb(thing, x);
                }
            }
            Item::Mod(x) => {
                for cb in mod_callbacks {
                    cb(thing, x);
                }
            }
            Item::Struct(x) => {
                for cb in struct_callbacks {
                    cb(thing, x);
                }
            }
            Item::Use(x) => {
                for cb in use_callbacks {
                    cb(thing, x);
                }
            }
            Item::Const(x) => {
                for cb in const_callbacks {
                    cb(thing, x);
                }
            }
            Item::Impl(x) => {
                for cb in impl_callbacks {
                    cb(thing, x);
                }
            }
            Item::ExternCrate(x) => {
                for cb in extern_crate_callbacks {
                    cb(thing, x);
                }
            }
            x => {
                for cb in fallback_cb {
                    cb(thing, x);
                }
            },
        }
    }
}

pub fn iterate_mod_def(
    module: &mut HiraModule2,
    mod_def: &mut ItemMod,
    fn_callbacks: &[fn(&mut HiraModule2, &mut ItemFn)],
    struct_callbacks: &[fn(&mut HiraModule2, &mut ItemStruct)],
    use_callbacks: &[fn(&mut HiraModule2, &mut ItemUse)],
    mod_callbacks: &[fn(&mut HiraModule2, &mut ItemMod)],
    const_callbacks: &[fn(&mut HiraModule2, &mut ItemConst)],
    extern_crate_callbacks: &[fn(&mut HiraModule2, &mut ItemExternCrate)],
    impl_callbacks: &[fn(&mut HiraModule2, &mut ItemImpl)],
    fallback_cb: fn(&mut HiraModule2, &mut Item),
) {
    module.name = get_ident_string(&mod_def.ident);
    module.is_pub = match mod_def.vis {
        Visibility::Public(_) => true,
        _ => false,
    };

    iterate_mod_def_generic(module, mod_def,
        fn_callbacks,
        struct_callbacks,
        use_callbacks,
        mod_callbacks,
        const_callbacks,
        extern_crate_callbacks,
        impl_callbacks,
        &[fallback_cb],
    );
    let cfgs = extract_hiracfgs(&mut mod_def.attrs, None);
    module.hiracfgs.extend(cfgs);
    // remove attributes, since we dont want to try to compile #[hira]
    mod_def.attrs.clear();
    module.contents = mod_def.to_token_stream().to_string();
}

pub fn get_ident_string(id: &Ident) -> String {
    let mut s = id.to_string();
    remove_surrounding_quotes(&mut s);
    s
}

pub fn remove_surrounding_quotes(s: &mut String) {
    while s.starts_with('"') && s.ends_with('"') && s.len() > 1 {
        s.remove(0);
        s.pop();
    }
}

pub fn attr_ends_in(attr: &Attribute, searchstr: &str) -> bool {
    let path = match &attr.meta {
        Meta::Path(p) => {
            p
        }
        Meta::List(l) => {
            &l.path
        }
        Meta::NameValue(n) => {
            &n.path
        }
    };
    let mut path_string = path.to_token_stream().to_string();
    remove_surrounding_quotes(&mut path_string);
    path_string.ends_with(searchstr)
}

pub fn has_attr_that_ends_in(attributes: &[Attribute], searchstr: &str) -> bool {
    for attr in attributes.iter() {
        if attr_ends_in(attr, searchstr) { return true; }
    }
    false
}

pub fn extract_hiracfgs(attributes: &mut Vec<Attribute>, mut applied_to: Option<String>) -> Vec<Hiracfg> {
    let mut keep = vec![];
    let mut cfgs = vec![];
    for attr in attributes.drain(..) {
        let list = if let Meta::List(l) = &attr.meta {
            let mut path_string = l.path.to_token_stream().to_string();
            remove_surrounding_quotes(&mut path_string);
            if path_string.ends_with("hiracfg") {
                l
            } else {
                keep.push(attr);
                continue;
            }
        } else {
            keep.push(attr);
            continue;
        };
        let mut first = None;
        let mut second = None;
        for token in list.tokens.clone().into_iter() {
            let idstr = match &token {
                TokenTree::Ident(id) => get_ident_string(id),
                TokenTree::Literal(s) => {
                    let mut s = s.to_string();
                    remove_surrounding_quotes(&mut s);
                    s
                }
                _ => continue,
            };
            if first.is_none() {
                first = Some(idstr);
                continue;
            }
            if second.is_none() {
                second = Some(idstr);
            } else {
                break;
            }
        }
        let cfg = match (first, second) {
            (Some(k), x) => {
                Hiracfg {
                    key: k,
                    value: x,
                    applied_to: applied_to.take().unwrap_or_default(),
                }
            }
            _ => continue,
        };
        cfgs.push(cfg);
    }
    *attributes = keep;
    cfgs
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

pub fn has_comment(attributes: &[Attribute], match_str: &str) -> bool {
    for attr in attributes.iter() {
        // a doc comment will be reported as #[doc = "..."]
        // therefore we only check for name value
        if let Meta::NameValue(nv) = &attr.meta {
            let s = nv.value.to_token_stream().to_string();
            if s.contains(match_str) {
                return true;
            }
        }

    }

    false
}

pub fn has_derive(meta: &Meta, name: &str) -> bool {
    let list = if let Meta::List(l) = meta {
        l
    } else {
        // derive macros must be Meta::List
        return false
    };
    // derive macros must be Paren
    match list.delimiter {
        syn::MacroDelimiter::Paren(_) => {},
        _ => return false,
    }

    let first_item = if let Some(f) = list.path.segments.first() {
        f
    } else {
        return false
    };
    let ident_str = get_ident_string(&first_item.ident);
    if ident_str != "derive" {
        return false;
    }
    for token in list.tokens.clone().into_iter() {
        match token {
            TokenTree::Ident(id) => {
                let id_str = get_ident_string(&id);
                if id_str == name {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
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

pub fn convert_to_snake_case(field: &str) -> String {
    let mut out = "".to_string();
    for c in field.chars() {
        if c.is_ascii_alphabetic() && c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c.to_ascii_lowercase());
        }
    }
    out
}

pub fn parse_as_module_item(stream: TokenStream) -> Result<ItemMod, TokenStream> {
    let mod_def = syn::parse2::<ItemMod>(stream)
        .map_err(|e| compiler_error(&format!("Failed to parse as ItemMod. Hira expects modules to be only applied to rust modules\n{:?}", e)))?;
    Ok(mod_def)
}

#[derive(PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum DependencyTypeName {
    Mod1Or2(String),
    Library(String),
}

#[derive(Debug)]
pub enum DependencyType {
    Mod1or2(DependencyConfig),
    Library(String),
}

#[derive(Debug)]
pub struct DependencyConfig {
    pub name: String,
    pub level: ModuleLevel,
    pub deps: Vec<DependencyType>,
}

impl DependencyConfig {
    pub fn config_calling_code(&self, first_config_ident: Ident) -> TokenStream {
        let item_name = &self.name;
        let item_name_ident = format_ident!("{}", item_name);
        let mut config_lets = vec![];
        let mut config_pass = vec![];
        let mut recursive = vec![];
        for (i, item) in self.deps.iter().enumerate() {
            let conf_name = format_ident!("conf_{}_{}", item_name, i);
            match item {
                DependencyType::Mod1or2(x) => {
                    let x_name = format_ident!("{}", x.name);
                    config_lets.push(quote!{ let mut #conf_name = #x_name::Input::default(); });
                    config_pass.push(quote!{ &mut #conf_name, });
                    recursive.push(x.config_calling_code(conf_name));
                }
                DependencyType::Library(x) => {
                    let x_field_name = convert_to_snake_case(x);
                    let x_name = format_ident!("{}", x_field_name);
                    config_lets.push(quote!{ library_obj.set_current_module(#item_name); });
                    config_pass.push(quote!{ &mut library_obj.#x_name, });
                }
            };
        }
        quote! {
            #(#config_lets)*
            #item_name_ident::config(&mut #first_config_ident, #(#config_pass)*);

            #(#recursive)*
        }
    }
}

/// given the full file contents, iterate it as a syn::File and
/// call the callback for every Module we encounter
pub fn iter_hira_modules(contents: &str, cb: &mut impl FnMut(ItemMod) -> Result<bool, TokenStream>) -> Result<(), TokenStream> {
    let synfile = syn::parse_file(contents)
        .map_err(|e| compiler_error(&format!("Failed to parse as rust file\n{}", e)))?;
    for item in synfile.items {
        if let Item::Mod(x) = item {
            // skip mod imports, we only care about mod definitions
            if x.content.is_none() {
                continue;
            }
            let should_continue = cb(x)?;
            if !should_continue {
                break;
            }
        }
    }
    Ok(())
}

pub fn ident_contains(id: &Ident, match_str: &str) -> bool {
    let s = get_ident_string(id);
    s.contains(match_str)
}

pub fn parse_documentation_from_attributes(attrs: &[Attribute]) -> String {
    let mut out = "".to_string();
    for att in attrs.iter() {
        if let Meta::NameValue(nv) = &att.meta {
            if !nv.path.segments.iter().any(|s| ident_contains(&s.ident, "doc")) {
                continue;
            }
            if let Expr::Lit(l) = &nv.value {
                if let syn::Lit::Str(s) = &l.lit {
                    let mut st = s.token().to_string();
                    remove_surrounding_quotes(&mut st);
                    out.push_str(&st);
                }
            }
        }
    }
    out.trim().to_string()
}

/// callback takes: field name, field type, field documentation
pub fn iter_fields(fields: &Fields, cb: &mut impl FnMut(String, String, String)) {
    let default_ident = Ident::new("a", Span::call_site());
    for field in fields.iter() {
        let ident = field.ident.as_ref().unwrap_or(&default_ident);
        let name = get_ident_string(&ident);
        let mut typ = field.ty.to_token_stream().to_string();
        remove_surrounding_quotes(&mut typ);
        let doc = parse_documentation_from_attributes(&field.attrs);
        cb(name, typ, doc);
    }
}

pub fn fill_dependency_config(hira_conf: &HiraConfig, name: &str, dep_contents: &mut Vec<TokenStream>) -> Result<DependencyConfig, TokenStream> {
    let dep_module = hira_conf.get_mod2(name)
        .ok_or(compiler_error(&format!("Failed to find module {}, but this module has not been loaded yet", name)))?;
    let mut out = DependencyConfig {
        name: name.to_string(),
        level: dep_module.level,
        deps: vec![],
    };
    // TODO: add deduplication logic here. lvl2 modules
    // can depend on other lvl2 modules so there could be a circular dependency.
    // which is fine! but we just have to ensure we dont emit the code multiple times.
    let contents_stream = TokenStream::from_str(&dep_module.contents)
        .map_err(|e| compiler_error(&format!("Failed to parse module {} as token stream\n{:?}", name, e)))?;
    dep_contents.push(contents_stream);

    for dep in dep_module.compile_dependencies.iter() {
        let dep_type = match dep {
            DependencyTypeName::Mod1Or2(s) => {
                let conf = fill_dependency_config(hira_conf, &s, dep_contents)?;
                DependencyType::Mod1or2(conf)
            }
            DependencyTypeName::Library(s) => {
                DependencyType::Library(s.clone())
            }
        };
        out.deps.push(dep_type);
    }
    Ok(out)
}

/// convenience function for testing. simply calls `parse_module_from_stream` underneath
pub fn parse_module_from_string<S: AsRef<str>>(s: S) -> Result<HiraModule2, TokenStream> {
    let stream = s.as_ref().parse::<TokenStream>()
        .map_err(|e| compiler_error(&format!("Failed to parse string as token stream {:?}", e)))?;
    parse_module_from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_snakecase_works() {
        let field_ty = "L0KvReader";
        assert_eq!(convert_to_snake_case(field_ty), "l0_kv_reader");
    }

    #[test]
    fn iterating_item_tree_works() {
        let tokens: TokenStream = "use hello::{world, something_else as xyz, third::*};".parse().unwrap();
        let item_tree = syn::parse2::<ItemUse>(tokens).unwrap();
        let mut outs = vec![];
        let mut past_names = vec![];
        iterate_item_tree(&mut past_names, &item_tree.tree, &mut |a, b, c| {
            outs.push((a.to_vec(), b, c));
        });
        assert_eq!(outs[0].0, &["hello", "world"]);
        assert_eq!(outs[0].1, None);
        assert_eq!(outs[0].2, false);

        assert_eq!(outs[1].0, &["hello", "something_else"]);
        assert_eq!(outs[1].1, Some("xyz".to_string()));
        assert_eq!(outs[1].2, false);

        assert_eq!(outs[2].0, &["hello", "third"]);
        assert_eq!(outs[2].1, None);
        assert_eq!(outs[2].2, true);
    }

    #[test]
    fn iterating_item_tree_works_self() {
        let tokens: TokenStream = "use self::some_module::some_thing;".parse().unwrap();
        let item_tree = syn::parse2::<ItemUse>(tokens).unwrap();
        let mut outs = vec![];
        let mut past_names = vec![];
        iterate_item_tree(&mut past_names, &item_tree.tree, &mut |a, b, c| {
            outs.push((a.to_vec(), b, c));
        });
        assert_eq!(outs[0].0, &["self", "some_module", "some_thing"]);
    }

    #[test]
    fn iterating_item_tree_works_crate() {
        let tokens: TokenStream = "use crate::some_module::some_thing;".parse().unwrap();
        let item_tree = syn::parse2::<ItemUse>(tokens).unwrap();
        let mut outs = vec![];
        let mut past_names = vec![];
        iterate_item_tree(&mut past_names, &item_tree.tree, &mut |a, b, c| {
            outs.push((a.to_vec(), b, c));
        });
        assert_eq!(outs[0].0, &["crate", "some_module", "some_thing"]);
    }

    #[test]
    fn iterating_item_tree_works_super() {
        let tokens: TokenStream = "use super::some_module::some_thing;".parse().unwrap();
        let item_tree = syn::parse2::<ItemUse>(tokens).unwrap();
        let mut outs = vec![];
        let mut past_names = vec![];
        iterate_item_tree(&mut past_names, &item_tree.tree, &mut |a, b, c| {
            outs.push((a.to_vec(), b, c));
        });
        assert_eq!(outs[0].0, &["super", "some_module", "some_thing"]);
    }

    #[test]
    fn iterating_item_tree_works_dotdotfirst() {
        let tokens: TokenStream = "use ::some_module::some_thing;".parse().unwrap();
        let item_tree = syn::parse2::<ItemUse>(tokens).unwrap();
        let mut outs = vec![];
        let mut past_names = vec![];
        iterate_item_tree(&mut past_names, &item_tree.tree, &mut |a, b, c| {
            outs.push((a.to_vec(), b, c));
        });
        assert_eq!(outs[0].0, &["some_module", "some_thing"]);
    }

    #[test]
    fn extracting_attrs_works() {
        let tokens: TokenStream = "#[hiracfg(helloworld)]pub const X: u32 = 2;".parse().unwrap();
        let mut item_tree = syn::parse2::<ItemConst>(tokens).unwrap();
        let out = extract_hiracfgs(&mut item_tree.attrs, None);
        assert!(item_tree.attrs.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "helloworld");

        let tokens: TokenStream = "#[hiracfg(key = \"value\")]pub const X: u32 = 2;".parse().unwrap();
        let mut item_tree = syn::parse2::<ItemConst>(tokens).unwrap();
        let out = extract_hiracfgs(&mut item_tree.attrs, None);
        assert!(item_tree.attrs.is_empty());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key, "key");
        assert_eq!(out[0].value, Some("value".to_string()));
    }
}
