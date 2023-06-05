use std::{str::FromStr};

use proc_macro2::{TokenStream};
use wasm_type_gen::*;
use quote::{quote, format_ident};

use crate::{
    parsing::{
        DependencyConfig, fill_dependency_config,
    },
    HiraConfig,
    module_loading::{HiraModule2},
    level0::*,
};

#[derive(WasmTypeGen, Debug)]
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

#[derive(Debug)]
pub struct MapEntry<T> {
    pub key: String,
    pub lines: Vec<T>,
}

#[derive(WasmTypeGen, Debug)]
#[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
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
    dont_run_wasm: bool,
    custom_codegen_opts: Option<Vec<&str>>,
) -> Option<LibraryObj> {
    let _ = std::fs::create_dir_all(wasm_out_dir);
    let out_file = wasm_type_gen::compile_strings_to_wasm_with_extern_crates(
        code, extern_crates,
        wasm_out_dir, custom_codegen_opts
    ).expect("compilation error");
    if dont_run_wasm {
        return None;
    }
    let wasm_file = std::fs::read(out_file).expect("failed to read wasm binary");
    let out = run_wasm(&wasm_file, data_to_pass.to_binary_slice()).expect("runtime error running wasm");
    LibraryObj::from_binary_slice(out)
}


pub fn get_wasm_code_to_compile2(
    hira_conf: &HiraConfig,
    hira_module_lvl3: &HiraModule2,
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

    if cfg!(feature = "web") {
        return quote! {
            extern crate hira_base;
            extern crate serde_json;
            extern crate sapp_jsutils;
            extern crate #dep_crate_name;
            use hira_base::LibraryObj;
            use #dep_crate_name::*;

            #lvl3mod_tokens

            fn create_js_obj(typ: &str, err: String) -> sapp_jsutils::JsObject {
                let mut obj = serde_json::Map::new();
                obj.insert(typ.to_string(), serde_json::Value::String(err));
                let val = serde_json::Value::Object(obj);
                let err_ser = serde_json::to_string(&val).unwrap_or("{\"err\":\"failed to serialize error\"}".to_string());
                sapp_jsutils::JsObject::string(&err_ser)
            }

            #[no_mangle]
            pub extern "C" fn get_module_default() -> sapp_jsutils::JsObject {
                let default_conf = #mod2name::Input::default();
                match serde_json::to_string(&default_conf) {
                    Ok(s) => {
                        create_js_obj("ok", s)
                    }
                    Err(e) => {
                        create_js_obj("err", format!("Failed to serialize default Input\n{:?}", e))
                    }
                }
            }

            #[no_mangle]
            pub extern "C" fn run_module_config(lib_obj: sapp_jsutils::JsObject, conf0data: sapp_jsutils::JsObject) -> sapp_jsutils::JsObject {
                let mut lib_obj_str = String::new();
                lib_obj.to_string(&mut lib_obj_str);
                let mut conf0data_str = String::new();
                conf0data.to_string(&mut conf0data_str);

                let mut lib_obj_actual: LibraryObj = match serde_json::from_str(&lib_obj_str) {
                    Ok(s) => s,
                    Err(e) => {
                        return create_js_obj("err", format!("Failed to parse {} as LibraryObj\n{:?}", lib_obj_str, e));
                    }
                };
                let library_obj = &mut lib_obj_actual;
                let mut #conf0 = match serde_json::from_str(&conf0data_str) {
                    Ok(s) => s,
                    Err(e) => {
                        return create_js_obj("err", format!("Failed to parse {} as Input\n{:?}", conf0data_str, e));
                    }
                };
                // let mut #conf0 = #mod2name::Input::default();
                #mod3name::config(&mut #conf0);

                #mod2_calling_code

                match serde_json::to_string(library_obj) {
                    Ok(s) => {
                        create_js_obj("ok", s)
                    }
                    Err(e) => {
                        create_js_obj("err", format!("Failed to serialize library obj\n{:?}", e))
                    }
                }
            }
        };
    }

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
