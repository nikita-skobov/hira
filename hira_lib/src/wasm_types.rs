use std::str::FromStr;

use proc_macro2::TokenStream;
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

use crate::parsing::{is_public, remove_surrounding_quotes, rename_ident, set_visibility};

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

#[derive(WasmTypeGen, Debug)]
pub struct SharedOutputEntry {
    pub filename: String,
    pub label: String,
    pub line: String,
    pub unique: bool,
    pub after: Option<String>,
}

#[derive(WasmTypeGen, Debug, Default)]
pub struct LibraryObj {
    pub compiler_error_message: String,
    pub add_code_after: Vec<String>,
    /// crate_name is read only. modifying this has no effect.
    pub crate_name: String,
    pub user_data: UserData,
    pub shared_output_data: Vec<SharedOutputEntry>,
    /// a shared key/value store for accessing data across wasm module invocations.
    /// this can be both used by you as a module writer, as well as optionally exposing
    /// this to users by providing this data in your exported struct.
    /// NOTE: this is append only + read. a wasm module cannot modify/delete existing key/value pairs
    /// but it CAN read them, as well as append new ones.
    pub shared_state: std::collections::HashMap<String, String>,
    /// names of dependencies that the user has specified in their Cargo.toml.
    /// NOTE: these are read only.
    pub dependencies: Vec<String>,
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
        let user_data = lib_obj.user_data;
        match (self, user_data) {
            (InputType::Struct(x), UserData::Struct { name, is_pub, .. }) => {
                rename_ident(&mut x.ident, &name);
                set_visibility(&mut x.vis, is_pub);
            }
            (InputType::Function(x), UserData::Function { name, is_pub, .. }) => {
                rename_ident(&mut x.sig.ident, &name);
                set_visibility(&mut x.vis, is_pub);
            }
            (InputType::GlobalVar(GlobalVariable::Constant(x)), UserData::GlobalVariable { name, is_pub, .. }) => {
                rename_ident(&mut x.ident, &name);
                set_visibility(&mut x.vis, is_pub);
            }
            (InputType::GlobalVar(GlobalVariable::Static(x)), UserData::GlobalVariable { name, is_pub, .. }) => {
                rename_ident(&mut x.ident, &name);
                set_visibility(&mut x.vis, is_pub);
            }
            (InputType::Module(x), UserData::Module { name, is_pub, append_to_body, .. }) => {
                rename_ident(&mut x.ident, &name);
                if let Some((_, content)) = &mut x.content {
                    for line in append_to_body {
                        let s = match TokenStream::from_str(&line) {
                            Ok(s) => s,
                            Err(e) => panic!("Module {wasm_module_name} attempted to add an invalid line to mod def {name}\nError:\n{:?}", e),
                        };
                        let c = match syn::parse2::<Item>(s) {
                            Ok(s) => s,
                            Err(e) => panic!("Module {wasm_module_name} attempted to add an invalid line to mod def {name}\nError:\n{:?}", e),
                        };
                        content.push(c);
                    }
                }
                set_visibility(&mut x.vis, is_pub);
            }
            _ => {}
        }
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


impl LibraryObj {
    pub fn handle_file_ops(&mut self, _wasm_module_name: &str, _user_type_name: &str) -> Result<(), String> {
        // output_shared_files(wasm_module_name, to_map_entry(std::mem::take(&mut self.shared_output_data)))
        todo!()
    }
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

#[output_and_stringify_basic_const(LIBRARY_OBJ_IMPL)]
impl LibraryObj {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            compiler_error_message: Default::default(),
            add_code_after: Default::default(),
            crate_name: Default::default(),
            user_data: UserData::new(),
            shared_output_data: Default::default(),
            shared_state: Default::default(),
            dependencies: Default::default(),
        }
    }
    #[allow(dead_code)]
    fn compile_error(&mut self, err_msg: &str) {
        self.compiler_error_message = err_msg.into();
    }
    /// a simple utility for generating 'hashes' based on some arbitrary input.
    /// Read more about adler32 here: https://en.wikipedia.org/wiki/Adler-32
    #[allow(dead_code)]
    fn adler32(&mut self, data: &[u8]) -> u32 {
        let mod_adler = 65521;
        let mut a: u32 = 1;
        let mut b: u32 = 0;
        for &byte in data {
            a = (a + byte as u32) % mod_adler;
            b = (b + a) % mod_adler;
        }
        (b << 16) | a
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
    fn append_to_file(&mut self, name: &str, label: &str, line: String) {
        self.shared_output_data.push(SharedOutputEntry { label: label.into(), line, filename: name.into(), unique: false, after: None });
    }

    /// same as append_to_file, but the line will be unique within the label
    #[allow(dead_code)]
    fn append_to_file_unique(&mut self, name: &str, label: &str, line: String) {
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
    fn append_to_line(&mut self, name: &str, label: &str, search_str: String, after: String) {
        self.shared_output_data.push(SharedOutputEntry { label: label.into(), line: search_str, filename: name.into(), unique: false, after: Some(after) });
    }
}

#[output_and_stringify_basic_const(USER_DATA_IMPL)]
impl UserData {
    #[allow(dead_code)]
    fn new() -> Self {
        UserData::Missing
    }
    /// Get the name of the user's data that they put this macro over.
    /// for example `struct MyStruct { ... }` returns "MyStruct"
    /// 
    /// or `pub fn helloworld(a: u32) { ... }` returns "helloworld"
    /// Can rename the user's data type by modifying this string directly
    #[allow(dead_code)]
    fn get_name(&mut self) -> &mut String {
        match self {
            UserData::Struct { name, .. } => name,
            UserData::Function { name, .. } => name,
            UserData::Module { name, .. } => name,
            UserData::GlobalVariable { name, .. } => name,
            UserData::Match { name, .. } => name,
            UserData::Missing => unreachable!(),
        }
    }
    /// Returns a bool of whether or not the user marked their data as pub or not.
    /// Can set this value to true or false depending on your module's purpose.
    #[allow(dead_code)]
    fn get_public_vis(&mut self) -> &mut bool {
        match self {
            UserData::Struct { is_pub, .. } => is_pub,
            UserData::Function { is_pub, .. } => is_pub,
            UserData::Module { is_pub, .. } => is_pub,
            UserData::GlobalVariable { is_pub, .. } => is_pub,
            UserData::Match { is_pub, .. } => is_pub,
            UserData::Missing => unreachable!(),
        }
    }
}

pub fn user_data_impl() -> &'static str {
    USER_DATA_IMPL
}

pub fn lib_obj_impl() -> &'static str {
    LIBRARY_OBJ_IMPL
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
    wasm_out_dir: Option<String>,
    out_name_hash: &str,
    wasm_source: String,
    add_to_source: Option<String>,
    data_to_pass: &LibraryObj,
) -> Option<LibraryObj> {
    let out_file = compile_string_to_wasm(out_name_hash, &wasm_source, add_to_source, wasm_out_dir).expect("compilation error");
    let wasm_file = std::fs::read(out_file).expect("failed to read wasm binary");
    let out = run_wasm(&wasm_file, data_to_pass.to_binary_slice()).expect("runtime error running wasm");
    LibraryObj::from_binary_slice(out)
}

/// given the user's wasm module, the wasm module's exported name,
/// the user's attribute (their callback), and the required modules for this module,
/// create an output that is ready to be passed into wasm_type_gen's compilation helper
pub fn get_wasm_code_to_compile(
    module_name: &str,
    exported_name: &str,
    attr: &TokenStream,
    parsed_wasm_code: File,
    required_hira_mods: Vec<TokenStream>,
) -> (String, Option<String>) {
    let mut add_to_code = LibraryObj::include_in_rs_wasm();
    add_to_code.push_str(LibraryObj::gen_entrypoint());
    add_to_code.push_str(WASM_PARSING_TRAIT_STR);
    add_to_code.push_str(lib_obj_impl());
    add_to_code.push_str(user_data_impl());

    // TODO: support default user function callbacks
    // let users_fn_def = if let Some(defcb) = get_default_user_cb(module_name) {
    //     quote! {
    //         pub fn users_fn(data: &mut #module_name_ident::#exported_name) {
    //             let cb = #attr;
    //             let default_cb = #defcb;
    //             default_cb(data);
    //             cb(data);
    //         }
    //     }
    // } else {
    // };

    let module_name_ident = format_ident!("{module_name}");
    let exported_name = format_ident!("{exported_name}");
    let users_fn = quote! {
        pub fn users_fn(data: &mut #module_name_ident::#exported_name) {
            let cb = #attr;
            cb(data);
        }
    };

    let final_wasm_source = quote! {
        pub fn wasm_main(library_obj: &mut LibraryObj) {
            let _ = #module_name_ident::wasm_entrypoint(library_obj, users_fn);
        }
        mod #module_name_ident {
            use super::LibraryObj;
            use super::UserData;

            #(#required_hira_mods)*

            #parsed_wasm_code
        }
        #users_fn
    };

    (final_wasm_source.to_string(), Some(add_to_code))
}