use std::collections::HashMap;
use std::sync::Mutex;
use std::{path::PathBuf, io::Write};
use std::str::FromStr;
use syn::Item;
use toml::Table;

use proc_macro2::{Ident, TokenStream};
use syn::{
    Type,
    parse_file,
    ItemFn,
    ItemStruct,
    ItemStatic,
    ItemConst,
    ItemMod,
    Visibility,
    token::Pub,
    ExprMatch,
};
use quote::{quote, format_ident, ToTokens};
use wasm_type_gen::*;

pub mod parsing;
pub mod module_loading;

pub const HIRA_DIR_NAME: &'static str = "hira";
pub const HIRA_WASM_DIR_NAME: &'static str = "wasm_out";
pub const HIRA_GEN_DIR_NAME: &'static str = "generated";
pub const HIRA_MODULES_DIR_NAME: &'static str = "modules";


#[derive(Default, Debug)]
pub struct HiraConfig {
    pub cargo_directory: String,
    pub hira_directory: String,
    pub modules_directory: String,
    pub wasm_directory: String,
    pub gen_directory: String,

    pub loaded_modules: HashMap<String, module_loading::HiraModule>,
}

impl HiraConfig {
    fn new() -> Self {
        let mut out = Self::default();
        out.set_directories();
        out
    }

    fn set_directories(&mut self) {
        let base_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(".".into());
        self.cargo_directory = base_dir;
        self.hira_directory = format!("{}/{HIRA_DIR_NAME}", self.cargo_directory);
        self.modules_directory = format!("{}/{HIRA_MODULES_DIR_NAME}", self.hira_directory);
        self.wasm_directory = format!("{}/{HIRA_WASM_DIR_NAME}", self.hira_directory);
        self.gen_directory = format!("{}/{HIRA_GEN_DIR_NAME}", self.hira_directory);
    }
}

static mut PERSISTED_DATA: Mutex<Option<HiraConfig>> = Mutex::new(None);

pub fn use_hira_config(mut cb: impl FnMut(&mut HiraConfig)) {
    unsafe {
        if let Ok(mut lock) = PERSISTED_DATA.lock() {
            if lock.is_none() {
                *lock = Some(HiraConfig::new());
            }
            if let Some(config) = lock.as_mut() {
                cb(config);
            }
        }
    }
}
