use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use module_loading::HiraModule;
use toml::Table;

pub mod parsing;
pub mod module_loading;
pub mod wasm_types;

use crate::module_loading::load_module;

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

    pub known_cargo_dependencies: HashSet<String>,
    pub loaded_modules: HashMap<String, module_loading::HiraModule>,
}

impl HiraConfig {
    fn new() -> Self {
        let mut out = Self::default();
        out.set_directories();
        out.load_cargo_toml();
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

    pub fn get_module(&mut self, module_name: &str) -> Result<&HiraModule, String> {
        let split_count = module_name.split("_").into_iter().count();
        if split_count != 2 {
            return Err(format!("{:?} is not a valid module name. Module names must have 1 underscore separating a namespace and a name", module_name));
        }
        if !self.loaded_modules.contains_key(module_name) {
            let x = load_module(self, module_name.to_string())?;
            self.loaded_modules.insert(x.name.clone(), x);
        }
        if let Some(m) = self.loaded_modules.get(module_name) {
            return Ok(m);
        }
        Err(format!("Failed to resolve module '{}' even after loading...", module_name))
    }

    fn load_cargo_toml(&mut self) {
        let file_path = format!("{}/Cargo.toml", self.cargo_directory);
        let cargo_file_str = if let Ok(file_str) = std::fs::read_to_string(file_path) {
            file_str
        } else {
            return
        };
        let value = cargo_file_str.parse::<Table>().unwrap();
        let mut dependencies = HashSet::new();        
        if let Some(deps) = value.get("dependencies") {
            if let toml::Value::Table(deps) = deps {
                for (key, _) in deps {
                    dependencies.insert(key.clone());
                }
            }
        }
        self.known_cargo_dependencies = dependencies;
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
