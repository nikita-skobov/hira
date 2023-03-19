use std::{collections::{HashMap, HashSet}, fmt::Debug, str::FromStr};

use proc_macro2::{TokenStream, Delimiter, TokenTree};
use rhai::{Engine, AST, Scope, Map, Dynamic, EvalAltResult, Array};
use serde::{Serialize, Deserialize};
use base64::{Engine as _, engine::general_purpose};

use crate::{resources::{AttributeValue, FuncDef, ModDef, RESOURCES, add_post_cmd, get_deploy_region, MatchDef, add_build_cmd, add_param_value, BUILD_BUCKET}, variables};

#[derive(Clone, Debug)]
pub enum RhaiObject {
    Mod { settings: GlobalSettings, def: ModDef },
    Func { settings: GlobalSettings, def: FuncDef },
    Match { settings: GlobalSettings, def: MatchDef },
}

impl RhaiObject {
    pub fn build(self) -> (GlobalSettings, TokenStream) {
        let (settings, stream) = match self {
            RhaiObject::Mod { settings, def } => (settings, def.build()),
            RhaiObject::Func { settings, def } => (settings, def.build()),
            RhaiObject::Match { settings, def } => (settings, def.build()),
        };
        let mut out_stream = TokenStream::new();
        for before in &settings.add_code_before {
            out_stream.extend(before.clone());
        }
        out_stream.extend(stream);
        for outside_stream in &settings.add_code_after {
            out_stream.extend(outside_stream.clone());
        }
        (settings, out_stream)
    }
    pub fn get_settings<T, F: FnMut(&mut GlobalSettings) -> T>(&mut self, mut cb: F) -> T {
        match self {
            RhaiObject::Mod { settings, .. } |
            RhaiObject::Match { settings, .. } |
            RhaiObject::Func { settings, .. } => {
                cb(settings)
            }
        }
    }
    pub fn assert_mod(self) -> ModDef {
        match self {
            RhaiObject::Mod { def, .. } => def,
            x => panic!("Expected module but found {:?}", x),
        }
    }
    pub fn assert_func(self) -> FuncDef {
        match self {
            RhaiObject::Func { def, .. } => def,
            x => panic!("Expected func but found {:?}", x),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GlobalSettings {
    pub add_code_after: Vec<TokenStream>,
    pub add_code_before: Vec<TokenStream>,
}

pub static mut CODE_ADDED_AFTER: Option<HashSet<String>> = None;

impl RhaiObject {
    pub fn build_engine(&self, eng: &mut Engine) {
        // always provide these functions: they are valid regardless of
        // mod, or func defs.
        eng.register_fn("add_to_cfn", |s: &str| {
            // TODO: i wonder if theres a better API for this.. its incredibly hacky...
            unsafe {
                RESOURCES.push(s.into());
            }
        });
        eng.register_fn("add_post_build_command", |s: &str| {
            // TODO: theres ways to make this safer. for eg: only allow some types of
            // commands such as cargo build and cargo run. and enforce it being separated by a cfg()...
            add_post_cmd(s);
        });
        eng.register_fn("add_build_command", |s: &str| {
            add_build_cmd(s);
        });
        eng.register_fn("add_param_value", |pkey: &str, pval: &str| {
            add_param_value((pkey, pval));
        });
        eng.register_fn("get_build_bucket", || -> String {
            let build_bucket = unsafe {&BUILD_BUCKET};
            if build_bucket.is_empty() {
                panic!("Must provide a build_bucket via hira::set_build_bucket!(\"bucketname-here\");");
            }
            build_bucket.clone()
        });
        eng.register_fn("get_bin_name", || -> String {
            let mut bin_name = "".to_string();
            for (key, value) in std::env::vars() {
                if key == "CARGO_BIN_NAME" || key == "CARGO_CRATE_NAME" {
                    bin_name = value;
                }
            }
            bin_name
        });
        eng.register_fn("add_code_after", |obj: &mut RhaiObject, s: &str| -> Result<(), String> {
            obj.get_settings(|settings| {
                // important: ensure no functions added after are the same otherwise the build
                // will break. this is convenient for the module writes so that they
                // can always output the code they want, and we prevent them from
                // creating duplicates by accident.
                unsafe {
                    if CODE_ADDED_AFTER.is_none() {
                        CODE_ADDED_AFTER = Some(HashSet::new());
                    }
                    if let Some(code_set) = &mut CODE_ADDED_AFTER {
                        if code_set.contains(s) {
                            return Ok(());
                        }
                        code_set.insert(s.into());
                    }
                }
                let stream = TokenStream::from_str(s)
                    .map_err(|e| format!("Error creating TokenStream in `add_code_after` from {s}. {e}"))?;
                settings.add_code_after.push(stream);
                Ok(())
            })
        });
        eng.register_fn("add_code_before", |obj: &mut RhaiObject, s: &str| -> Result<(), String> {
            obj.get_settings(|settings| {
                let stream = TokenStream::from_str(s)
                    .map_err(|e| format!("Error creating TokenStream in `add_code_before` from {s}. {e}"))?;
                settings.add_code_before.push(stream);
                Ok(())
            })
        });
        // also should be included for both types, but has different implementations:
        eng.register_fn("rename", |obj: &mut RhaiObject, s: &str| {
            match obj {
                RhaiObject::Mod { def, .. } => {
                    def.set_module_name(s);
                }
                RhaiObject::Func { def, .. } => {
                    def.set_func_name(s);
                }
                RhaiObject::Match { def, .. } => {
                    def.set_name(s);
                }
            }
        });
        eng.register_fn("set_global_const", |obj: &mut RhaiObject, key: &str, val: &str| {
            let mod_name = match obj {
                RhaiObject::Mod { def, .. } => {
                    def.get_module_name()
                }
                RhaiObject::Func { def, .. } => {
                    def.get_func_name()
                }
                RhaiObject::Match { def, .. } => {
                    def.get_name()
                }
            };
            let module_key = format!("{mod_name}::{key}");
            variables::set_const(&module_key, val)
        });
        eng.register_fn("get_name", |obj: &mut RhaiObject| -> String {
            match obj {
                RhaiObject::Mod { def, .. } => {
                    def.get_module_name()
                }
                RhaiObject::Func { def, .. } => {
                    def.get_func_name()
                }
                RhaiObject::Match { def, .. } => {
                    def.get_name()
                }
            }
        });
        // specific to functions:
        if let RhaiObject::Func { .. } = &self {
            eng.register_fn("is_const", |obj: &mut RhaiObject| -> bool {
                match &obj {
                    RhaiObject::Func { def, .. } => def.fn_const_ident.is_some(),
                    _ => false,
                }
            });
            eng.register_fn("is_async", |obj: &mut RhaiObject| -> bool {
                match &obj {
                    RhaiObject::Func { def, .. } => def.fn_async_ident.is_some(),
                    _ => false,
                }
            });
            eng.register_fn("is_unsafe", |obj: &mut RhaiObject| -> bool {
                match &obj {
                    RhaiObject::Func { def, .. } => def.fn_unsafe_ident.is_some(),
                    _ => false,
                }
            });
            eng.register_fn("is_pub", |obj: &mut RhaiObject| -> bool {
                match &obj {
                    RhaiObject::Func { def, .. } => def.fn_pub_ident.is_some(),
                    _ => false,
                }
            });
            eng.register_fn("get_return_type", |obj: &mut RhaiObject| -> String {
                match &obj {
                    RhaiObject::Func { def, .. } => def.get_return_type(),
                    _ => "".into(),
                }
            });
            eng.register_fn("set_return_type", |obj: &mut RhaiObject, s: &str| {
                match obj {
                    RhaiObject::Func { def, .. } => {
                        def.set_return_type(s);
                    }
                    _ => {},
                }
            });
            eng.register_fn("get_parameters", |obj: &mut RhaiObject| -> Array {
                match obj {
                    RhaiObject::Func { def, .. } => {
                        if def.params.is_empty() {
                            def.build_params();
                        }
                        let mut out = vec![];
                        for (param_name, param_type) in &def.params {
                            let mut map = Map::new();
                            map.insert("param_name".into(), param_name.into());
                            map.insert("param_type".into(), param_type.into());
                            out.push(Dynamic::from_map(map));
                        }
                        Array::from(out)
                    },
                    _ => Array::from(vec![])
                }
            });
        }
        // specific to modules:
        if let RhaiObject::Mod { .. } = &self {
            eng.register_fn("add_code_inside", |obj: &mut RhaiObject, s: &str| -> Result<(), Box<EvalAltResult>> {
                if let RhaiObject::Mod { def, .. } = obj {
                    let stream = match TokenStream::from_str(s) {
                        Ok(o) => o,
                        Err(e) => {
                            return Err(format!("Error creating TokenStream in `add_code_inside` from {s}. {e}").into());
                        }
                    };
                    def.add_to_body(stream);
                }
                Ok(())
            });
            eng.register_fn("contains_tokens", |obj: &mut RhaiObject, s: &str| -> Result<bool, Box<EvalAltResult>> {
                if let RhaiObject::Mod { def, .. } = obj {
                    let stream = match TokenStream::from_str(s) {
                        Ok(o) => o,
                        Err(e) => {
                            return Err(format!("Error creating TokenStream in `contains_tokens` from {s}. {e}").into());
                        }
                    };
                    return Ok(def.contains_tokens(stream));
                }
                Ok(false)
            });
            eng.register_fn("get_encapsulated_value", |obj: &mut RhaiObject, s: &str| -> Result<String, Box<EvalAltResult>> {
                if let RhaiObject::Mod { def, .. } = obj {
                    match def.get_encapsulated_value(s) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            return Err(format!("Error getting encapsulated value of {s}\n{e}").into());
                        }
                    }
                }
                Err("get_encapsulated_value is only valid for mod defs.".into())
            });
        }
        // specific to match statements:
        if let RhaiObject::Match { .. } = &self {
            eng.register_fn("get_match_content", |obj: &mut RhaiObject| -> Map {
                let mut map = Map::new();
                if let RhaiObject::Match { def, .. } = obj {
                    let match_against = def.match_against.clone();
                    let mut out = vec![];
                    for (match_part, result_part) in &def.match_statements {
                        let mut inner_obj = Map::new();
                        inner_obj.insert("match".into(), match_part.clone().into());
                        inner_obj.insert("result".into(), result_part.clone().into());
                        out.push(inner_obj);
                    }
                    map.insert("match_body".into(), out.into());
                    map.insert("match_against".into(), match_against.into());
                }
                map
            });
        }
    }
}

pub struct ModuleInput {
    pub module_name: String,
    pub module_json: HashMap<String, AttributeValue>,
}

#[derive(Serialize, Deserialize)]
pub struct GitHubResponse {
    pub content: String,
    pub encoding: String,
}

/// given a module name, find the module script and load it.
pub fn resolve_module(module_name: &str) -> Result<(Engine, AST), String> {
    let script = if let Some((module_namespace, module_name)) = module_name.split_once(":") {
        // first, check if the module was previously downloaded
        let path = format!("./hira/modules/{module_namespace}/{module_name}.rhai");
        let should_download = match std::fs::metadata(&path) {
            Ok(_) => false,
            Err(_) => true,
        };
        if should_download {
            let url = format!("https://api.github.com/repos/nikita-skobov/hira/contents/registry/{module_namespace}/{module_name}.rhai");
            let body: GitHubResponse = match ureq::get(&url).call() {
                Ok(resp) => match resp.into_json() {
                    Ok(r) => r,
                    Err(e) => {
                        return Err(format!("Unsuccessful response to fetch module {}:{} from github\n{:#?}", module_namespace, module_name, e));
                    }
                },
                Err(e) => {
                    return Err(format!("Failed to request module {}:{} from github\n{:#?}", module_namespace, module_name, e));
                }
            };
            let script = if body.encoding == "base64" {
                let body = body.content.replace("\n","");
                let decoded = general_purpose::STANDARD.decode(body).map_err(|e| e.to_string())?;
                String::from_utf8_lossy(&decoded).to_string()
            } else {
                body.content
            };
            // save it to disk:
            let module_dir = &format!("./hira/modules/{module_namespace}");
            if let Err(e) = std::fs::create_dir_all(&module_dir) {
                // just warn, but keep trying...
                eprintln!("Error creating module directory {module_dir}\n{e}");
            }
            if let Err(e) = std::fs::write(path, script.as_bytes()) {
                eprintln!("Error saving module {module_namespace}:{module_name} to hira/modules\n{e}");
            }
            script
        } else {
            // we already have it, so just read it:
            match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    return Err(format!("Failed to load module '{module_name}' from file system. {e}"));
                }
            }
        }
    } else {
        // if it's not a namespaced module, then it should be a path to the module script.
        match std::fs::read_to_string(module_name) {
            Ok(s) => s,
            Err(e) => {
                return Err(format!("Failed to load module '{module_name}' from file system. {e}"));
            }
        }
    };

    let mut engine = Engine::new();
    engine.set_max_expr_depths(0, 0);
    let ast = match engine.compile(script) {
        Ok(a) => a,
        Err(e) => {
            return Err(format!("Failed to parse module '{module_name}' as rhai script. {e}"));
        }
    };

    Ok((engine, ast))
}

pub fn attribute_map_to_rhai_map(attr_map: &HashMap<String, AttributeValue>) -> Dynamic {
    let map = AttributeValue::Map(attr_map.clone());
    attribute_map_to_rhai_map_inner(&map)
}

pub fn attribute_map_to_rhai_map_inner(attr_val: &AttributeValue) -> Dynamic {
    match attr_val {
        AttributeValue::Str(s) => {
            Dynamic::from(s.clone())
        }
        AttributeValue::List(list) => {
            let mut arr = vec![];
            for item in list {
                arr.push(attribute_map_to_rhai_map_inner(item));
            }
            Dynamic::from_array(arr)
        }
        AttributeValue::Map(m) => {
            let mut map = Map::new();
            for (key, val) in m {
                map.insert(key.clone().into(), attribute_map_to_rhai_map_inner(val));
            }
            Dynamic::from_map(map)
        }
    }
}

pub fn create_module_scope(input: &ModuleInput) -> Scope {
    let mut out = Scope::new();
    // scope should contain metadata about this module invocation
    out.push("HIRA_MOD_NAME", input.module_name.clone());
    let rhai_map = attribute_map_to_rhai_map(&input.module_json);
    out.push("HIRA_MOD_INPUT", rhai_map);
    let region = get_deploy_region();
    out.push("HIRA_DEPLOY_REGION", region.clone());
    out
}

pub fn run_module(input: &ModuleInput, fn_name: &str, item: RhaiObject) -> Result<RhaiObject, String> {
    let (mut eng, ast) = resolve_module(&input.module_name)?;
    let mut scope = create_module_scope(input);
    item.build_engine(&mut eng);

    let mut has_mod_macro_fn = false;
    let desired_param_count = 1;
    for fndef in ast.iter_functions() {
        if fndef.name == fn_name {
            has_mod_macro_fn = true;
            if fndef.params.len() != desired_param_count {
                return Err(format!("fn {fn_name}() {{}} was found but it takes {} parameters, expected {}", fndef.params.len(), desired_param_count));
            }
        }
    }
    if !has_mod_macro_fn {
        return Err(format!("hira::module '{}' is missing a fn {fn_name}(x) {{}} function.", input.module_name));
    }

    // necessary to allow the scope to persist if the user
    // wants to call different functions within their module.
    let options = rhai::CallFnOptions::default()
        .rewind_scope(false).eval_ast(false);
    match eng.call_fn_with_options::<RhaiObject>(options, &mut scope, &ast, fn_name, (item, )) {
        Ok(m) => {
            Ok(m)
        }
        Err(e) => {
            match *e {
                rhai::EvalAltResult::ErrorMismatchOutputType(_, _, _) => {
                    Err(format!("Error in module '{}'. fn {fn_name}(x) {{ }} must return the first input parameter", input.module_name))
                }
                _ => Err(format!("Error running module '{}': {}", input.module_name, e)),
            }
        }
    }
}

pub fn get_module_input(attr: TokenStream) -> Result<ModuleInput, String> {
    // macro invocation must looks like:
    // [my_macro("name-of-module", { "pseudo-json-data": "here" })]
    let generic_err = "Ensure you are invoking this macro in this format: `hira::module(\"macro_name\", {\"data\":\"here\"})`";
    let mut iter = attr.into_iter();
    let next = iter.next().ok_or_else(|| format!("Missing attribute stream. {generic_err}"))?;
    let mut module_name = if let TokenTree::Literal(s) = next {
        s.to_string()
    } else {
        return Err(format!("First arg to hira::module must be a string literal. Instead found {:?}. {generic_err}", next))
    };

    // strip surrounding quotes
    loop {
        if module_name.starts_with('"') && module_name.ends_with('"') {
            module_name.remove(0);
            module_name.pop();
        } else {
            break;
        }
    }

    let punct_err = format!("Must have punctuation after first arg to hira::module. {generic_err}");
    let next = iter.next().ok_or_else(|| punct_err.clone())?;
    if let TokenTree::Punct(p) = next {
        if p.as_char() != ',' {
            return Err(punct_err);
        }
    } else {
        return Err(punct_err);
    }
    // assert that it must be a group:
    let next = iter.next().ok_or_else(|| format!("Missing second parameter to macro attributes. {generic_err}"))?;
    let brace_group = if let TokenTree::Group(g) = &next {
        if g.delimiter() != Delimiter::Brace {
            return Err(format!("Arg after [hira::module(\"{module_name}\", )] must be in object format '{{}}'. {generic_err}"));
        }
        let mut out = TokenStream::new();
        out.extend([next]);
        out
    } else {
        return Err(format!("Arg after [hira::module(\"{module_name}\", )] must be in object format '{{}}'. {generic_err}"));
    };

    let attribute_val = AttributeValue::from(brace_group);
    let module_json = match attribute_val {
        AttributeValue::Map(m) => m,
        x => return Err(format!("Expected a map as second hira::module argument. Instead found {:?}. {generic_err}", x)),
    };

    Ok(ModuleInput {
        module_json,
        module_name,
    })
}

#[cfg(test)]
mod test {
    use crate::resources::{parse_func_def_safe, parse_mod_def_safe};

    use super::*;

    #[test]
    fn can_rename() {
        let input = ModuleInput {
            module_name: "./src/module_scripting/test_fixtures/can_rename.rhai".into(),
            module_json: Default::default(),
        };
        let rust_code = TokenStream::from_str("fn myfunc() {}").unwrap();
        let def = parse_func_def_safe(rust_code, false).unwrap();
        let obj = run_module(&input, "func_macro", RhaiObject::Func { settings: Default::default(), def }).unwrap();
        let def = obj.assert_func();
        assert_eq!(def.get_func_name(), "renamed");

        let rust_code = TokenStream::from_str("mod mymod {}").unwrap();
        let def = parse_mod_def_safe(rust_code).unwrap();
        let obj = run_module(&input, "mod_macro", RhaiObject::Mod { settings: Default::default(), def }).unwrap();
        let def = obj.assert_mod();
        assert_eq!(def.get_module_name(), "renamed");
    }

    #[test]
    fn can_add_code_before_and_after() {
        let input = ModuleInput {
            module_name: "./src/module_scripting/test_fixtures/can_add_code_before_and_after.rhai".into(),
            module_json: Default::default(),
        };
        let rust_code = TokenStream::from_str("fn myfunc() {}").unwrap();
        let def = parse_func_def_safe(rust_code, false).unwrap();
        let obj = run_module(&input, "func_macro", RhaiObject::Func { settings: Default::default(), def }).unwrap();
        let (_, token_stream) = obj.build();
        let s = token_stream.to_string();
        assert_eq!(s, "# [cfg (hello)] fn myfunc () { } fn generatedfn1 () { }");

        let rust_code = TokenStream::from_str("mod mymod {}").unwrap();
        let def = parse_mod_def_safe(rust_code).unwrap();
        let obj = run_module(&input, "mod_macro", RhaiObject::Mod { settings: Default::default(), def }).unwrap();
        let (_, token_stream) = obj.build();
        let s = token_stream.to_string();
        assert_eq!(s, "# [cfg (hello)] mod mymod { } fn generatedfn2 () { }");
    }

    #[test]
    fn can_add_code_after_multiple_times_without_breaking() {
        let input = ModuleInput {
            module_name: "./src/module_scripting/test_fixtures/can_add_code_after_multiple_times_without_breaking.rhai".into(),
            module_json: Default::default(),
        };
        let rust_code = TokenStream::from_str("fn myfunc() {}").unwrap();
        let def = parse_func_def_safe(rust_code, false).unwrap();
        let obj = run_module(&input, "func_macro", RhaiObject::Func { settings: Default::default(), def }).unwrap();
        let (_, token_stream) = obj.build();
        let s = token_stream.to_string();
        assert_eq!(s, "fn myfunc () { } fn generatedfn () { }");

        let rust_code = TokenStream::from_str("mod mymod {}").unwrap();
        let def = parse_mod_def_safe(rust_code).unwrap();
        let obj = run_module(&input, "mod_macro", RhaiObject::Mod { settings: Default::default(), def }).unwrap();
        let (_, token_stream) = obj.build();
        let s = token_stream.to_string();
        // we already added generatedfn(), so it shouldnt appear again here.
        assert_eq!(s, "mod mymod { }");
    }

    #[test]
    fn can_add_code_inside_modules() {
        let input = ModuleInput {
            module_name: "./src/module_scripting/test_fixtures/can_add_code_inside_modules.rhai".into(),
            module_json: Default::default(),
        };

        let rust_code = TokenStream::from_str("mod mymod {}").unwrap();
        let def = parse_mod_def_safe(rust_code).unwrap();
        let obj = run_module(&input, "mod_macro", RhaiObject::Mod { settings: Default::default(), def }).unwrap();
        let (_, token_stream) = obj.build();
        let s = token_stream.to_string();
        assert_eq!(s, "mod mymod { pub fn generatedfunc () { } }");
    }
}
