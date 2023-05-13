use std::{str::FromStr, collections::HashSet};

use proc_macro2::{TokenStream, Ident};
use wasm_type_gen::*;
use syn::{
    ItemFn,
    ItemStruct,
    ItemStatic,
    ItemConst,
    ItemMod,
    ExprMatch, Item, File,
};
use quote::{quote, format_ident, ToTokens};

use crate::{
    parsing::{
        is_public, remove_surrounding_quotes, rename_ident, set_visibility, compiler_error, convert_to_snake_case,
        DependencyConfig, DependencyType, DependencyTypeName, fill_dependency_config,
    },
    HiraConfig,
    module_loading::{HiraModule2, OutputType},
    level0::*,
};

#[derive(WasmTypeGen, Debug, Default)]
pub struct FunctionSignature {
    pub name: String,
    pub is_pub: bool,
    pub is_async: bool,
    pub is_unsafe: bool,
    pub is_const: bool,
    pub inputs: Vec<UserInput>,
    pub return_ty: String,
}

// this is the data the end user passed to the macro, and we serialize it
// and pass it to the wasm module that the user specified
#[derive(WasmTypeGen, Debug)]
pub enum UserData {
    /// fields are read only. modifying them in your wasm_module has no effect.
    Struct { name: String, is_pub: bool, fields: Vec<UserField> },
    /// inputs are read only. modifying them in your wasm_module has no effect.
    Function { name: String, is_pub: bool, is_async: bool, inputs: Vec<UserInput>, return_ty: String },
    /// The body is a stringified version of the module body. You can use this to search through and do minimal
    /// static analysis. The existing body is read only, however you can append to the body by adding lines
    /// after the existing body via `append_to_body`.
    Module { name: String, is_pub: bool, body: String, append_to_body: Vec<String> },
    GlobalVariable { name: String, is_pub: bool, },
    Match { name: String, expr: Vec<String>, is_pub: bool, arms: Vec<MatchArm> },
    Missing,
}
impl Default for UserData {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(Debug)]
pub struct MapEntry<T> {
    pub key: String,
    pub lines: Vec<T>,
}

#[derive(WasmTypeGen, Debug)]
pub struct MatchArm {
    pub pattern: Vec<Option<String>>,
    pub expr: String,
}

#[derive(WasmTypeGen, Debug)]
pub struct UserField {
    /// only relevant for struct fields. not applicable to function params.
    pub is_public: bool,
    pub name: String,
    pub ty: String,
}

#[derive(WasmTypeGen, Debug)]
pub struct UserInput {
    /// only relevant for input params to a function. not applicable to struct fields.
    pub is_self: bool,
    pub name: String,
    pub ty: String,
}

#[derive(WasmTypeGen, Debug)]
pub struct FileOut {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum GlobalVariable {
    Constant(ItemConst),
    Static(ItemStatic),
}

#[derive(Debug)]
pub enum InputType {
    Struct(ItemStruct),
    Function(ItemFn),
    GlobalVar(GlobalVariable),
    Module(ItemMod),
    Match(ExprMatch),
}

impl InputType {
    pub fn get_name(&self) -> String {
        match self {
            InputType::Struct(di) => di.ident.to_string(),
            InputType::Function(fi) => fi.sig.ident.to_string(),
            InputType::Module(mi) => mi.ident.to_string(),
            InputType::GlobalVar(gi) => match gi {
                GlobalVariable::Constant(c) => c.ident.to_string(),
                GlobalVariable::Static(c) => c.ident.to_string(),
            }
            InputType::Match(mi) => mi.expr.to_token_stream().to_string(),
        }
    }
    /// use_name is only necessary for Match input types. for match statements
    /// we hide the match inside a function, otherwise most match statements arent valid
    /// in a const context, but const contexts is the only way we can conveniently read + parse them
    pub fn back_to_stream(self, use_name: &str) -> proc_macro2::TokenStream {
        match self {
            InputType::Struct(s) => s.into_token_stream(),
            InputType::Function(f) => f.into_token_stream(),
            InputType::GlobalVar(g) => match g {
                GlobalVariable::Constant(c) => c.into_token_stream(),
                GlobalVariable::Static(s) => s.into_token_stream(),
            }
            InputType::Match(m) => {
                let use_name_ident = format_ident!("{use_name}");
                quote! {
                    fn #use_name_ident() {
                        #m;
                    }
                }
            }
            InputType::Module(m) => m.into_token_stream(),
        }
    }
}


impl InputType {
    pub fn apply_library_obj_changes(&mut self, lib_obj: LibraryObj, wasm_module_name: &str) {
        // let user_data = lib_obj.user_data;
        // match (self, user_data) {
        //     (InputType::Struct(x), UserData::Struct { name, is_pub, .. }) => {
        //         rename_ident(&mut x.ident, &name);
        //         set_visibility(&mut x.vis, is_pub);
        //     }
        //     (InputType::Function(x), UserData::Function { name, is_pub, .. }) => {
        //         rename_ident(&mut x.sig.ident, &name);
        //         set_visibility(&mut x.vis, is_pub);
        //     }
        //     (InputType::GlobalVar(GlobalVariable::Constant(x)), UserData::GlobalVariable { name, is_pub, .. }) => {
        //         rename_ident(&mut x.ident, &name);
        //         set_visibility(&mut x.vis, is_pub);
        //     }
        //     (InputType::GlobalVar(GlobalVariable::Static(x)), UserData::GlobalVariable { name, is_pub, .. }) => {
        //         rename_ident(&mut x.ident, &name);
        //         set_visibility(&mut x.vis, is_pub);
        //     }
        //     (InputType::Module(x), UserData::Module { name, is_pub, append_to_body, .. }) => {
        //         rename_ident(&mut x.ident, &name);
        //         if let Some((_, content)) = &mut x.content {
        //             for line in append_to_body {
        //                 let s = match TokenStream::from_str(&line) {
        //                     Ok(s) => s,
        //                     Err(e) => panic!("Module {wasm_module_name} attempted to add an invalid line to mod def {name}\nError:\n{:?}", e),
        //                 };
        //                 let c = match syn::parse2::<Item>(s) {
        //                     Ok(s) => s,
        //                     Err(e) => panic!("Module {wasm_module_name} attempted to add an invalid line to mod def {name}\nError:\n{:?}", e),
        //                 };
        //                 content.push(c);
        //             }
        //         }
        //         set_visibility(&mut x.vis, is_pub);
        //     }
        //     _ => {}
        // }
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

impl From<&InputType> for UserData {
    fn from(value: &InputType) -> Self {
        let name = value.get_name();
        match value {
            InputType::Struct(x) => {
                let mut fields = vec![];
                for field in x.fields.iter() {
                    let usr_field = UserField {
                        is_public: is_public(&field.vis),
                        name: field.ident.as_ref().map(|i| i.to_string()).unwrap_or_default(),
                        ty: field.ty.to_token_stream().to_string(),
                    };
                    fields.push(usr_field);
                }
                Self::Struct { name, is_pub: is_public(&x.vis), fields }
            },
            InputType::Function(x) => {
                let mut inputs = vec![];
                for input in x.sig.inputs.iter() {
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
                let return_ty = match &x.sig.output {
                    syn::ReturnType::Default => "".into(),
                    syn::ReturnType::Type(_, b) => b.to_token_stream().to_string(),
                };
                Self::Function { name, is_pub: is_public(&x.vis), inputs, is_async: x.sig.asyncness.is_some(), return_ty }
            }
            InputType::GlobalVar(GlobalVariable::Constant(x)) => {
                Self::GlobalVariable { name, is_pub: is_public(&x.vis) }
            }
            InputType::GlobalVar(GlobalVariable::Static(x)) => {
                Self::GlobalVariable { name, is_pub: is_public(&x.vis) }
            }
            InputType::Module(x) => {
                let body = match &x.content {
                    Some((_, items)) => {
                        let mut out = "".to_string();
                        for item in items {
                            out.push_str(&item.to_token_stream().to_string());
                            out.push('\n');
                        }
                        out
                    }
                    None => "".into(),
                };
                Self::Module { name, is_pub: is_public(&x.vis), body, append_to_body: vec![] }
            }
            InputType::Match(x) => {
                let mut arms = vec![];
                for arm in x.arms.iter() {
                    let pattern = match &arm.pat {
                        syn::Pat::Tuple(tpl) => {
                            let mut out = vec![];
                            for thing in tpl.elems.iter() {
                                match thing {
                                    syn::Pat::Wild(_) => {
                                        out.push(None);
                                    }
                                    x => {
                                        let mut s = x.to_token_stream().to_string();
                                        remove_surrounding_quotes(&mut s);
                                        out.push(Some(s));
                                    }
                                }
                            }
                            out
                        }
                        syn::Pat::Wild(_) => {
                            vec![None]
                        }
                        x => {
                            let mut s = x.to_token_stream().to_string();
                            remove_surrounding_quotes(&mut s);
                            vec![Some(s)]
                        }
                    };
                    let mut expr = arm.body.to_token_stream().to_string();
                    remove_surrounding_quotes(&mut expr);
                    arms.push(MatchArm { pattern, expr })
                }
                let mut expr = vec![];
                match &*x.expr {
                    syn::Expr::Tuple(tpl) => {
                        for item in tpl.elems.iter() {
                            let mut s = item.to_token_stream().to_string();
                            remove_surrounding_quotes(&mut s);
                            expr.push(s);    
                        }
                    }
                    x => {
                        let mut s = x.to_token_stream().to_string();
                        remove_surrounding_quotes(&mut s);
                        expr.push(s);
                    }
                }
                Self::Match { name, expr, is_pub: false, arms }
            }
        }
    }
}

pub fn to_map_entry(data: Vec<SharedOutputEntry>) -> Vec<MapEntry<MapEntry<(bool, String, Option<String>)>>> {
    let mut map_entries: Vec<MapEntry<MapEntry<(bool, String, Option<String>)>>> = vec![];
    for d in data {
        if let Some(m) = map_entries.iter_mut().find(|x| x.key == d.filename) {
            if let Some(m) = m.lines.iter_mut().find(|x| x.key == d.label) {
                m.lines.push((d.unique, d.line, d.after));
            } else {
                m.lines.push(MapEntry { key: d.label, lines: vec![(d.unique, d.line, d.after)] });
            }
        } else {
            map_entries.push(MapEntry { key: d.filename, lines: vec![MapEntry {
                key: d.label,
                lines: vec![(d.unique, d.line, d.after)],
            }] })
        }
    }
    map_entries
}

/// TODO: should this fn be allowed to panic???
pub fn get_wasm_output(
    wasm_out_dir: &str,
    code: &[(String, String)],
    extern_crates: &[String],
    data_to_pass: &LibraryObj,
) -> Option<LibraryObj> {
    let _ = std::fs::create_dir_all(wasm_out_dir);
    let out_file = wasm_type_gen::compile_strings_to_wasm_with_extern_crates(code, extern_crates, wasm_out_dir).expect("compilation error");
    let wasm_file = std::fs::read(out_file).expect("failed to read wasm binary");
    let out = run_wasm(&wasm_file, data_to_pass.to_binary_slice()).expect("runtime error running wasm");
    LibraryObj::from_binary_slice(out)
}


pub fn get_wasm_code_to_compile2(
    hira_conf: &HiraConfig,
    hira_module_lvl3: &HiraModule2
) -> Result<[(String, String); 3], TokenStream> {
    let dependency_name = format!("dependencies_{}", hira_module_lvl3.name);
    let mut dependency_mod_defs = vec![];

    let l2_dep_name = hira_module_lvl3.level3_get_depends_on(hira_module_lvl3.lvl3_module_depends_on.as_ref())?;
    let dependency_config = fill_dependency_config(hira_conf, &l2_dep_name, &mut dependency_mod_defs)?;

    let hira_base_code = hira_conf.hira_base_code.clone();
    let module_code = quote! {
        extern crate hira_base;
        use hira_base::*;

        #(#dependency_mod_defs)*
    };
    let users_code = get_wasm_code_to_compile_lvl3(
        hira_module_lvl3.name.clone(), hira_module_lvl3.contents.clone(),
        dependency_config, &dependency_name,
    );

    let module_code = module_code.to_string();
    let users_code = users_code.to_string();

    Ok([
        ("hira_base".to_string(), hira_base_code),
        (dependency_name, module_code),
        (hira_module_lvl3.name.to_string(), users_code),
    ])
}

pub fn get_wasm_code_to_compile_lvl3(
    lvl3module_name: String,
    lvl3module_def: String,
    lvl2module: DependencyConfig,
    dependency_crate_name: &String,
) -> TokenStream {
    let mod3name = format_ident!("{}", lvl3module_name);
    let mod2name = format_ident!("{}", lvl2module.name);
    let conf0 = format_ident!("conf_0");
    let dep_crate_name = format_ident!("{}", dependency_crate_name);

    let mod2_calling_code = lvl2module.config_calling_code(conf0.clone());
    let lvl3mod_tokens = TokenStream::from_str(&lvl3module_def).expect("Failed to parse lvl3 module def as tokens");

    quote! {
        extern crate hira_base;
        extern crate #dep_crate_name;
        use hira_base::LibraryObj;
        use #dep_crate_name::*;

        #lvl3mod_tokens

        #[no_mangle]
        pub fn wasm_main(library_obj: &mut LibraryObj) {
            let mut #conf0 = #mod2name::Input::default();
            #mod3name::config(&mut #conf0);

            #mod2_calling_code
        }

        extern "C" {
            fn get_entrypoint_alloc_size() -> u32;
            fn get_entrypoint_data(ptr : * const u8, len : u32);
            fn set_entrypoint_data(ptr : * const u8, len : u32);
        }

        #[no_mangle]
        pub extern fn wasm_entrypoint() -> u32 {
            let mut input_obj = unsafe {
                let len = get_entrypoint_alloc_size() as usize ; let mut data : Vec <
                u8 > = Vec :: with_capacity(len) ; data.set_len(len) ; let ptr =
                data.as_ptr() ; let len = data.len() ;
                get_entrypoint_data(ptr, len as _) ; match LibraryObj ::
                from_binary_slice(data) { Some(s) => s, None => return 1, }
            };
            unsafe {
                let _ = wasm_main(& mut input_obj) ;
                let out_data = input_obj.to_binary_slice() ; let ptr =
                out_data.as_ptr() ; let len = out_data.len() ;
                set_entrypoint_data(ptr, len as _) ;
            }
            0
        }
    }
}



/// given the user's wasm module, the wasm module's exported name,
/// the user's attribute (their callback), and the required modules for this module,
/// create an output that is ready to be passed into wasm_type_gen's compilation helper
pub fn get_wasm_code_to_compile(
    hira_base_code: String,
    module_name: &str,
    users_item_name: &str,
    exported_name: &str,
    attr: &TokenStream,
    parsed_wasm_code: File,
    required_hira_mods: Vec<TokenStream>,
    default_cb: Option<String>,
) -> [(String, String); 3] {

    let module_code = quote! {
        extern crate hira_base;
        use hira_base::LibraryObj;
        use hira_base::UserData;

        #(#required_hira_mods)*

        #parsed_wasm_code
    };

    let mut default_cb_stream = None;
    if let Some(defcb) = default_cb {
        default_cb_stream = TokenStream::from_str(&defcb).ok();
    }
    let module_name_ident = format_ident!("{module_name}");
    let exported_name = format_ident!("{exported_name}");
    let users_fn = if let Some(defcb) = default_cb_stream {
        quote! {
            pub fn users_fn(data: &mut #module_name_ident::#exported_name) {
                let cb = #attr;
                let default_cb = #defcb;
                default_cb(data);
                cb(data);
            }
        }
    } else {
        quote! {
            pub fn users_fn(data: &mut #module_name_ident::#exported_name) {
                let cb = #attr;
                cb(data);
            }
        }
    };
    let users_code = quote! {
        extern crate hira_base;
        extern crate #module_name_ident;
        use hira_base::LibraryObj;

        #users_fn

        #[no_mangle]
        pub fn wasm_main(library_obj: &mut LibraryObj) {
            let _ = #module_name_ident::wasm_entrypoint(library_obj, users_fn);
        }

        extern "C" {
            fn get_entrypoint_alloc_size() -> u32;
            fn get_entrypoint_data(ptr : * const u8, len : u32);
            fn set_entrypoint_data(ptr : * const u8, len : u32);
        }

        #[no_mangle]
        pub extern fn wasm_entrypoint() -> u32 {
            let mut input_obj = unsafe {
                let len = get_entrypoint_alloc_size() as usize ; let mut data : Vec <
                u8 > = Vec :: with_capacity(len) ; data.set_len(len) ; let ptr =
                data.as_ptr() ; let len = data.len() ;
                get_entrypoint_data(ptr, len as _) ; match LibraryObj ::
                from_binary_slice(data) { Some(s) => s, None => return 1, }
            };
            unsafe {
                let _ = wasm_main(& mut input_obj) ;
                let out_data = input_obj.to_binary_slice() ; let ptr =
                out_data.as_ptr() ; let len = out_data.len() ;
                set_entrypoint_data(ptr, len as _) ;
            }
            0
        }
    };

    let module_code = module_code.to_string();
    let users_code = users_code.to_string();

    [
        ("hira_base".to_string(), hira_base_code),
        (module_name.to_string(), module_code),
        (users_item_name.to_string(), users_code),
    ]
}
