//! module w/ helper functions related to parsing from
//! token streams into hira specific structures
//! 

use std::str::FromStr;

use proc_macro2::{
    TokenStream,
    TokenTree, Ident,
};
use quote::{quote, ToTokens, format_ident};
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
    Expr, ItemUse, Visibility, token::Pub, ItemMacro, ItemImpl
};

use crate::{module_loading::{HiraModule, HiraModule2, ModuleLevel}, wasm_types::{InputType, GlobalVariable}, HiraConfig};

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

pub fn iterate_mod_def(
    module: &mut HiraModule2,
    mod_def: &mut ItemMod,
    fn_callbacks: &[fn(&mut HiraModule2, &mut ItemFn)],
    struct_callbacks: &[fn(&mut HiraModule2, &mut ItemStruct)],
    use_callbacks: &[fn(&mut HiraModule2, &mut ItemUse)],
    mod_callbacks: &[fn(&mut HiraModule2, &mut ItemMod)],
) {
    module.name = get_ident_string(&mod_def.ident);
    module.is_pub = match mod_def.vis {
        Visibility::Public(_) => true,
        _ => false,
    };

    let mut default_vec = vec![];
    let content = mod_def.content.as_mut().map(|x| &mut x.1).unwrap_or(&mut default_vec);
    for item in content {
        match item {
            Item::Fn(x) => {
                for cb in fn_callbacks {
                    cb(module, x);
                }
            }
            Item::Mod(x) => {
                for cb in mod_callbacks {
                    cb(module, x);
                }
            }
            Item::Struct(x) => {
                for cb in struct_callbacks {
                    cb(module, x);
                }
            }
            Item::Use(x) => {
                for cb in use_callbacks {
                    cb(module, x);
                }
            }
            _ => {},
        }
    }

    module.contents = mod_def.to_token_stream().to_string();
}

pub fn get_ident_string(id: &Ident) -> String {
    let mut s = id.to_string();
    remove_surrounding_quotes(&mut s);
    s
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
    macro_callbacks: &[fn(&mut HiraModule, &ItemMacro) -> bool],
    static_callbacks: &[fn(&mut HiraModule, &ItemStatic) -> bool],
    struct_callbacks: &[fn(&mut HiraModule, &ItemStruct) -> bool],
    trait_callbacks: &[fn(&mut HiraModule, &ItemTrait) -> bool],
    union_callbacks: &[fn(&mut HiraModule, &ItemUnion) -> bool],
    impl_callbacks: &[fn(&mut HiraModule, &ItemImpl) -> bool],
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
            Item::Impl(x) => {
                for cb in impl_callbacks {
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
            Item::Macro(x) => {
                for cb in macro_callbacks {
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


#[derive(PartialEq, Eq, Hash, Debug)]
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
                    config_pass.push(quote!{ &mut library_obj.#x_name, });
                }
            };
        }
        quote! {
            #(#config_lets)*
            library_obj.l0_core.set_current_module(#item_name);
            #item_name_ident::config(&mut #first_config_ident, #(#config_pass)*);

            #(#recursive)*
        }
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
}
