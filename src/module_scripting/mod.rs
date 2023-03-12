use std::collections::HashMap;

use proc_macro::{TokenStream, Delimiter};
use rhai::{Engine, AST, Scope, Map, Dynamic, ImmutableString};

use crate::resources::{AttributeValue, FuncDef, ModDef};


pub struct ModuleInput {
    pub module_name: String,
    pub module_json: HashMap<String, AttributeValue>,
}

/// given a module name, find the module script and load it.
pub fn resolve_module(module_name: &str) -> Result<(Engine, AST), String> {
    if let Some((_module_namespace, _module_name)) = module_name.split_once(":") {
        // TODO: handle some kind of module system via an id, rather than a file path.
        // in the future maybe people can register modules somehow.
        // but for now we will just say its unimplemented, and only allow modules via file paths.
        todo!()
    }

    // if it's not a namespaced module, then it should be a path to the module script.
    let script = match std::fs::read_to_string(module_name) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!("Failed to load module '{module_name}' from file system. {e}"));
        }
    };

    let engine = Engine::new();
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
    out
}

pub fn run_module_mod_def(input: &ModuleInput, mod_def: &ModDef) -> Result<(), String> {
    let (eng, ast) = resolve_module(&input.module_name)?;
    let mut scope = create_module_scope(input);
    match eng.run_ast_with_scope(&mut scope, &ast) {
        Ok(_) => Ok(()),
        Err(e) => {
            return Err(format!("Failed to run module {}. {e}", input.module_name));
        }
    }
}

pub fn run_module_func_def(input: &ModuleInput, func_def: &FuncDef) -> Result<(), String> {
    let (eng, ast) = resolve_module(&input.module_name)?;
    let mut scope = create_module_scope(input);
    match eng.run_ast_with_scope(&mut scope, &ast) {
        Ok(_) => Ok(()),
        Err(e) => {
            return Err(format!("Failed to run module {}. {e}", input.module_name));
        }
    }
}

pub fn get_module_input(attr: TokenStream) -> Result<ModuleInput, String> {
    // macro invocation must looks like:
    // [my_macro("name-of-module", { "pseudo-json-data": "here" })]
    println!("{:#?}", attr);
    let generic_err = "Ensure you are invoking this macro in this format: `hira::module(\"macro_name\", {\"data\":\"here\"})`";
    let mut iter = attr.into_iter();
    let next = iter.next().ok_or_else(|| format!("Missing attribute stream. {generic_err}"))?;
    let mut module_name = if let proc_macro::TokenTree::Literal(s) = next {
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
    if let proc_macro::TokenTree::Punct(p) = next {
        if p.as_char() != ',' {
            return Err(punct_err);
        }
    } else {
        return Err(punct_err);
    }
    // assert that it must be a group:
    let next = iter.next().ok_or_else(|| format!("Missing second parameter to macro attributes. {generic_err}"))?;
    let brace_group = if let proc_macro::TokenTree::Group(g) = &next {
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