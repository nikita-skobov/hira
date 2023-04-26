//! module w/ helper functions related to parsing from
//! token streams into hira specific structures
//! 

use std::str::FromStr;

use proc_macro2::{
    TokenStream,
    TokenTree,
};

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
