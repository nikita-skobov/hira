use std::collections::HashMap;

pub static mut DOT_ENV: Option<HashMap<String, String>> = None;
pub static mut LOADED_CONSTS: Option<HashMap<String, String>> = None;

pub fn load_dot_env_inner(path: String) {
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) => panic!("Failed to load .env file {}: {}", path, e),
    };
    let mut map = HashMap::new();
    for line in contents.lines() {
        if line.is_empty() || line.starts_with("#") {
            continue;
        }
        if let Some((key, val)) = line.split_once("=") {
            map.insert(key.into(), val.into());
        }
    }
    unsafe {
        DOT_ENV = Some(map);
    }
}

pub fn set_const(key: &str, val: &str) {
    unsafe {
        if LOADED_CONSTS.is_none() {
            LOADED_CONSTS = Some(HashMap::new());
        }
        if let Some(map) = &mut LOADED_CONSTS {
            map.insert(key.into(), val.into());
        }
    }
}

pub fn get_const(key: &str) -> Option<String> {
    unsafe {
        if let Some(map) = &LOADED_CONSTS {
            if let Some(val) = map.get(key) {
                return Some(val.clone());
            }
        }
    }
    None
}