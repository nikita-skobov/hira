use std::collections::HashMap;

pub use proc_macro::{TokenTree, TokenStream, Ident, Span, Punct, Delimiter, Group};

use crate::variables::get_const;


#[derive(Debug)]
pub enum AttributeValue {
    Str(String),
    List(Vec<AttributeValue>),
    Map(HashMap<String, AttributeValue>),
}

impl AttributeValue {
    pub fn assert_str(self, key: &str) -> String {
        match self {
            AttributeValue::Str(s) => s,
            _ => {
                panic!("Expected string type at {}. Instead found {:?}", key, self);
            }
        }
    }
    pub fn assert_map(self, key: &str) -> HashMap<String, AttributeValue> {
        match self {
            AttributeValue::Map(m) => m,
            _ => {
                panic!("Expected map type at {}. Instead found {:?}", key, self);
            }
        }
    }
    pub fn assert_list(self, key: &str) -> Vec<AttributeValue> {
        match self {
            AttributeValue::List(l) => l,
            _ => {
                panic!("Expected list type at {}. Instead found {:?}", key, self);
            }
        }
    }
}

pub fn get_attribute_value(token: TokenTree) -> AttributeValue {
    match token {
        // can either be a list or a map
        TokenTree::Group(g) => {
            match g.delimiter() {
                // this is an object
                proc_macro::Delimiter::Brace => {
                    let mut out = HashMap::new();
                    let mut iter = g.stream().into_iter();
                    let mut name_opt: Option<String> = None;
                    loop {
                        if let Some(next) = iter.next() {
                            if let Some(name) = name_opt.take() {
                                let val = get_attribute_value(next);
                                out.insert(name, val);
                                // get next token, it should either be a comma, or nonexistent
                                match iter.next() {
                                    Some(next) => {
                                        if let proc_macro::TokenTree::Punct(p) = next {
                                            if p.as_char() != ',' {
                                                panic!("Expected punctuation ',' after attribute value map. instead found {:?}", p);
                                            }
                                        } else {
                                            panic!("Expected punctuation ',' after attribute value map. instead found {:?}", next);
                                        }
                                    }
                                    // end of the object, break
                                    None => {
                                        break;
                                    }
                                }
                            } else {
                                // no name yet, we expect an identifier, or a literal
                                match next {
                                    TokenTree::Ident(i) => {
                                        name_opt = Some(i.to_string());
                                    }
                                    TokenTree::Literal(l) => {
                                        let mut s = l.to_string();
                                        if s.starts_with('"') && s.ends_with('"') {
                                            s.remove(0);
                                            s.pop();
                                        }
                                        name_opt = Some(s);
                                    }
                                    _ => {
                                        panic!("Expected an identifier in attribute value map. instead found {:?}", next);
                                    }
                                }
                                // after the name we expect a colon
                                let next = iter.next().expect("Expect punctuation after attribute value key");
                                if let proc_macro::TokenTree::Punct(p) = next {
                                    if p.as_char() != ':' {
                                        panic!("Expected punctuation ':' after attribute value key {:?}. Instead found {:?}", name_opt.unwrap(), p);
                                    }
                                } else {
                                    panic!("Expected punctuation ':' after attribute value key {:?}. Instead found {:?}", name_opt.unwrap(), next);
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    return AttributeValue::Map(out);
                }
                // this is a list
                proc_macro::Delimiter::Bracket => {
                    let mut iter = g.stream().into_iter();
                    let mut out = vec![];
                    loop {
                        if let Some(next) = iter.next() {
                            let val = get_attribute_value(next);
                            out.push(val);
                            // next should be a comma punct:
                            let next = match iter.next() {
                                Some(n) => n,
                                None => {
                                    // at the end of the list if we don't find a punctuation, that's the end of the list.
                                    break;
                                }
                            };
                            match next {
                                TokenTree::Punct(p) => {
                                    if p.as_char() != ',' {
                                        panic!("Expected punctuation ',' in attribute value list. Instead found {:?}", p);
                                    }
                                }
                                _ => {
                                    panic!("Expected punctuation ',' in attribute value list. Instead found {:?}", next); 
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    return AttributeValue::List(out);
                }
                _ => {
                    panic!("Attribute value is a group. Expected delimiter {{}} or []. instead found {:?}", g);
                }
            }
        }
        // this is a reference to a const variable that was previously loaded.
        // if it wasnt found, error.
        TokenTree::Ident(id) => {
            let id_key = id.to_string();
            if let Some(val) = get_const(&id_key) {
                return AttributeValue::Str(val);
            } else {
                panic!("Failed to find value for '{id_key}'. Make sure you load it as a proper const using const_from_dot_env!(). Or if this value is meant to be used as is, surround it in double quotes like as \"{id_key}\"");
            }
        }
        // also single values that we will treat as strings
        TokenTree::Literal(l) => {
            let mut s = l.to_string();
            if s.starts_with('"') && s.ends_with('"') {
                s.remove(0);
                s.pop();
            }
            return AttributeValue::Str(s);
        }
        // this is invalid
        TokenTree::Punct(p) => {
            panic!("Unexpected punctuation in attribute value {:?}", p);
        }
    }
}

pub fn parse_attributes(attr: TokenStream) -> AttributeValue {
    let mut iter = attr.into_iter();
    let next = match iter.next() {
        Some(n) => n,
        None => return AttributeValue::Map(HashMap::new()),
    };
    get_attribute_value(next)
}


fn expect_ident(s: &str) -> TokenTree {
    TokenTree::Ident(Ident::new(s, Span::call_site()))
}

fn expect_punct(c: char) -> TokenTree {
    TokenTree::Punct(Punct::new(c, proc_macro::Spacing::Alone))
}

fn expect_group(d: Delimiter) -> TokenTree {
    TokenTree::Group(Group::new(d, TokenStream::new()))
}

fn does_match_token(actual: &TokenTree, expected: &TokenTree, ignore_value: bool) -> Result<String, String> {
    match (actual, expected) {
        (TokenTree::Group(a), TokenTree::Group(b)) => {
            if a.delimiter() != b.delimiter() {
                panic!("Error parsing: Expected group with delimiter {:?}, Received {:?}", b.delimiter(), a);
            }
            Ok(match a.delimiter() {
                Delimiter::Parenthesis => "()".into(),
                Delimiter::Brace => "{}".into(),
                Delimiter::Bracket => "[]".into(),
                Delimiter::None => "".into(),
            })
        }
        (TokenTree::Ident(a), TokenTree::Ident(b)) => {
            // if we don't care the value inside, then we just care that the type matches
            if ignore_value { return Ok(a.to_string()) }
            let expected_str = b.to_string();
            if a.to_string() != expected_str {
                return Err(format!("Error parsing: Expected identifier {:?}, Received {:?}", b, a));
            }
            Ok(a.to_string())
        }
        (TokenTree::Punct(a), TokenTree::Punct(b)) => {
            // if we don't care the value inside, then we just care that the type matches
            if ignore_value { return Ok(a.to_string()) }
            let expected_char = b.as_char();
            if a.as_char() != expected_char {
                return Err(format!("Error parsing: Expected punctuation {:?}, Received {:?}", expected_char, a.as_char()));
            }
            Ok(a.to_string())
        }
        (TokenTree::Literal(a), TokenTree::Literal(b)) => {
            // if we don't care the value inside, then we just care that the type matches
            if ignore_value { return Ok(a.to_string()) }
            let expected_str = b.to_string();
            if a.to_string() != expected_str {
                return Err(format!("Error parsing: Expected literal {:?}, Received {:?}", expected_str, a.to_string()));
            }
            Ok(a.to_string())
        }
        // otherwise we know it's wrong because the type is wrong
        _ => {
            Err(format!("Error parsing: Expected {:?}, Received {:?}", expected, actual))
        }
    }
}

fn assert_token(actual: &TokenTree, expected: &TokenTree, ignore_value: bool) -> String {
    match does_match_token(actual, expected, ignore_value) {
        Ok(out) => out,
        Err(e) => panic!("{e}"),
    }
}

#[derive(Debug)]
pub struct FuncDef {
    pub fn_async_ident: Option<TokenTree>,
    pub fn_ident: TokenTree,
    pub fn_name: TokenTree,
    pub fn_params: TokenTree,
    pub fn_return_punct: Vec<TokenTree>,
    pub fn_return: Vec<TokenTree>,
    pub fn_body: TokenTree,
    pub params: Vec<(String, String)>,
}

impl Default for FuncDef {
    fn default() -> Self {
        Self {
            fn_async_ident: None,
            fn_ident: expect_ident("fn"),
            fn_name: expect_ident("fn"),
            fn_params: expect_ident("fn"),
            fn_return_punct: vec![],
            fn_return: vec![],
            fn_body: expect_ident("fn"),

            params: vec![],
        }
    }
}

impl FuncDef {
    pub fn build(self) -> TokenStream {
        let mut out = TokenStream::new();
        if let Some(async_ident) = self.fn_async_ident {
            out.extend([async_ident]);
        }
        out.extend([self.fn_ident]);
        out.extend([self.fn_name]);
        out.extend([self.fn_params]);
        out.extend(self.fn_return_punct);
        out.extend(self.fn_return);
        out.extend([self.fn_body]);
        out
    }
    pub fn build_params(&mut self) {
        let params = if let proc_macro::TokenTree::Group(g) = &self.fn_params {
            g
        } else {
            panic!("Somehow parameters is not a group?");
        };
        let mut iter = params.stream().into_iter();
        loop {
            let mut token = match iter.next() {
                Some(t) => t,
                None => { break }
            };
            // skip over punctuations of commas:
            if let TokenTree::Punct(p) = &token {
               if p.as_char() == ',' {
                    token = match iter.next() {
                        Some(t) => t,
                        None => { break }
                    };    
               } 
            }
            // name of the param
            let expect = expect_ident("fn");
            let name = assert_token(&token, &expect, true);
            let token = match iter.next() {
                Some(t) => t,
                None => { break }
            };
            // colon
            let expect = expect_punct(':');
            assert_token(&token, &expect, false);
            let token = match iter.next() {
                Some(t) => t,
                None => { break }
            };
            // type of the param
            let expect = expect_ident("fn");
            let val = assert_token(&token, &expect, true);
            self.params.push((name, val));
        }
    }
    pub fn get_return_type(&self) -> String {
        let mut stream = TokenStream::new();
        for token in &self.fn_return {
            stream.extend([token.clone()]);
        }
        stream.to_string()
    }
    pub fn change_func_name(&mut self, new_name: &str) {
        if let proc_macro::TokenTree::Ident(id) = &self.fn_name {
            let span = id.span();
            self.fn_name = TokenTree::Ident(Ident::new(new_name, span));
        } else {
            panic!("Expected fn_name to be an ident. instead found {:?}", self.fn_name);
        }
    }
    pub fn get_func_name(&self) -> String {
        if let proc_macro::TokenTree::Ident(id) = &self.fn_name {
            return id.to_string();
        } else {
            panic!("Expected fn_name to be an ident. instead found {:?}", self.fn_name);
        }
    }
    pub fn assert_num_params(&mut self, num: usize) {
        if self.params.is_empty() {
            self.build_params();
        }
        if self.params.len() != num {
            panic!("Expected function with {} parameters. Instead found {}", num, self.params.len());
        }
    }
    pub fn get_nth_param(&mut self, n: usize) -> (&str, &str) {
        if self.params.is_empty() {
            self.build_params();
        }
        match self.params.get(n) {
            Some((name, typ)) => (name, typ),
            None => panic!("Tried to access {}th param, but there are only {} parameters", n, self.params.len())
        }
    }
}

#[derive(Debug)]
pub struct ModDef {
    pub pub_ident: Option<TokenTree>,
    pub mod_ident: TokenTree,
    pub mod_name_ident: TokenTree,
    pub mod_body: TokenTree,
}

impl Default for ModDef {
    fn default() -> Self {
        Self {
            pub_ident: None,
            mod_ident: expect_ident("fn"),
            mod_name_ident: expect_ident("fn"),
            mod_body: expect_ident("fn"),
        }
    }
}

impl ModDef {
    pub fn build(self) -> TokenStream {
        let mut out = TokenStream::new();
        if let Some(id) = self.pub_ident {
            out.extend([id]);
        }
        out.extend([self.mod_ident]);
        out.extend([self.mod_name_ident]);
        out.extend([self.mod_body]);
        out
    }
    pub fn add_to_body(&mut self, add: TokenStream) {
        if let proc_macro::TokenTree::Group(g) = &mut self.mod_body {
            let mut old_body = g.stream();
            let span = g.span();
            old_body.extend(add);
            let mut new_group = Group::new(Delimiter::Brace, old_body);
            new_group.set_span(span);
            self.mod_body = TokenTree::Group(new_group);
        }
    }
    pub fn module_name(&self) -> String {
        if let proc_macro::TokenTree::Ident(id) = &self.mod_name_ident {
            return id.to_string();
        } else {
            panic!("Module missing name");
        }
    }
    pub fn contains_tokens(&self, token_stream: TokenStream) -> bool {
        let mut match_tokens = vec![];
        for token in token_stream {
            match_tokens.push(token);
        }
        let mut match_index = 0;
        let mut expect = &match_tokens[match_index];
        if let proc_macro::TokenTree::Group(g) = &self.mod_body {
            for token in g.stream() {
                if does_match_token(&token, &expect, false).is_ok() {
                    match_index += 1;
                    if match_index >= match_tokens.len() {
                        return true;
                    }
                    expect = &match_tokens[match_index];
                } else {
                    match_index = 0;
                    expect = &match_tokens[match_index];
                }
            }
        }
        false
    }
}

pub fn parse_mod_def(token_stream: TokenStream) -> ModDef {
    let mut out = ModDef::default();
    let mut iter = token_stream.into_iter();
    let generic_err = "Error parsing: Unexpected end of token stream. This can only be applied to modules. Are you sure you added this macro attribute to a module?";
    let mut next = iter.next().expect(generic_err);
    let mut expect = expect_ident("pub");
    let actual_ident = assert_token(&next, &expect, true);
    if actual_ident == "pub" {
        out.pub_ident = Some(next);
        next = iter.next().expect(generic_err);
        expect = expect_ident("mod");
        assert_token(&next, &expect, false);
        out.mod_ident = next;
    } else if actual_ident == "mod" {
        out.mod_ident = next;
    } else {
        panic!("Unexpected identifier parsing module: {:?}", next);
    }
    // we expect this to be the name of the module
    next = iter.next().expect(generic_err);
    assert_token(&next, &expect, true);
    out.mod_name_ident = next;
    // now we expect the mod body, so it should be a group
    expect = expect_group(Delimiter::Brace);
    next = iter.next().expect(generic_err);
    assert_token(&next, &expect, false);
    out.mod_body = next;
    out
}

pub fn parse_func_def(token_stream: TokenStream, assert_async: bool) -> FuncDef {
    let mut out = FuncDef::default();
    let mut expect = expect_ident("async");
    let mut iter = token_stream.into_iter();
    let generic_err = "Error parsing: Unexpected end of token stream. This can only be applied to functions. Are you sure you added this macro attribute to a function?";
    let mut next = iter.next().expect(generic_err);
    // this can either be fn or async
    let actual_ident = assert_token(&next, &expect, true);
    if actual_ident == "async" {
        out.fn_async_ident = Some(next);
        next = iter.next().expect(generic_err);
        expect = expect_ident("fn");
        assert_token(&next, &expect, false);
        out.fn_ident = next;
    } else {
        out.fn_ident = next;
    }
    if assert_async {
        if out.fn_async_ident.is_none() {
            panic!("This function must be async");
        }
    }
    expect = expect_ident("fn"); // we expect next to be the name of the function
    next = iter.next().expect(generic_err);
    assert_token(&next, &expect, true);
    out.fn_name = next;
    expect = expect_group(Delimiter::Parenthesis);
    next = iter.next().expect(generic_err);
    assert_token(&next, &expect, false);
    out.fn_params = next;
    next = iter.next().expect(generic_err);
    // next can either be punctuation for the return type, or the body of the function def
    match &next {
        TokenTree::Punct(p) => {
            if p.as_char() != '-' { panic!("Error parsing: Expected punctuation '-', instead found {:?}", p) }
            out.fn_return_punct.push(next);
            next = iter.next().expect(generic_err);
            if let proc_macro::TokenTree::Punct(p) = &next {
                if p.as_char() != '>' { panic!("Error parsing: Expected punctuation '-', instead found {:?}", p) }
            }
            out.fn_return_punct.push(next);
            // now we parse the return type.
            loop {
                next = iter.next().expect(generic_err);
                if let proc_macro::TokenTree::Group(g) = &next {
                    // if it's a group with delimiter Brace, that means
                    // it's the function body
                    if g.delimiter() == Delimiter::Brace {
                        out.fn_body = next;
                        break;
                    }
                }
                out.fn_return.push(next);
            }
        }
        TokenTree::Group(_) => {
            // this means there was no explicit return type
            out.fn_return = vec![];
            out.fn_body = next;
        }
        _ => {
            panic!("Error parsing: Expected return type for function. Instead found {:?}", next);
        }
    }

    out
}
