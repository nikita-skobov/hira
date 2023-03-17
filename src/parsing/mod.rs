use std::{collections::HashMap, str::FromStr};

pub use proc_macro2::{Spacing, TokenTree, TokenStream, Ident, Span, Punct, Delimiter, Group};

use super::variables::get_const;


#[derive(Debug, Clone)]
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

impl From<TokenStream> for AttributeValue {
    fn from(value: TokenStream) -> Self {
        parse_attributes(value)
    }
}

pub fn get_attribute_value(token: TokenTree) -> AttributeValue {
    match token {
        // can either be a list or a map
        TokenTree::Group(g) => {
            match g.delimiter() {
                // this is an object
                Delimiter::Brace => {
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
                                        if let TokenTree::Punct(p) = next {
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
                                if let TokenTree::Punct(p) = next {
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
                Delimiter::Bracket => {
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
    TokenTree::Punct(Punct::new(c, Spacing::Alone))
}

fn expect_group(d: Delimiter) -> TokenTree {
    TokenTree::Group(Group::new(d, TokenStream::new()))
}

fn does_match_token(actual: &TokenTree, expected: &TokenTree, ignore_value: bool) -> Result<String, String> {
    match (actual, expected) {
        (TokenTree::Group(a), TokenTree::Group(b)) => {
            if a.delimiter() != b.delimiter() {
                return Err(format!("Error parsing: Expected group with delimiter {:?}, Received {:?}", b.delimiter(), a));
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
    match assert_token_safe(actual, expected, ignore_value) {
        Ok(out) => out,
        Err(e) => panic!("{e}"),
    }
}

fn assert_token_safe(actual: &TokenTree, expected: &TokenTree, ignore_value: bool) -> Result<String, String> {
    does_match_token(actual, expected, ignore_value)
}

#[derive(Debug, Clone)]
pub struct MatchDef {
    pub pub_ident: Option<TokenTree>,
    pub const_ident: TokenTree,
    pub statement_name: String, // eg: const xyz: _ = match {}. the statement_name is "xyz"
    pub statement_name_ident: TokenTree,
    pub punct_ident: TokenTree,
    pub type_ident: TokenTree,
    pub equals_ident: TokenTree,
    pub match_ident: TokenTree,
    pub match_parens_ident: TokenTree,
    pub match_body: TokenTree,
    pub semicolon_ident: Option<TokenTree>,

    pub match_against: Vec<String>,
    pub match_statements: Vec<(Vec<String>, Vec<String>)>,
}

impl MatchDef {
    pub fn build(self) -> TokenStream {
        let mut out = TokenStream::new();
        if let Some(pub_ident) = self.pub_ident {
            out.extend([pub_ident]);
        }
        out.extend([self.const_ident]);
        out.extend([self.statement_name_ident]);
        out.extend([self.punct_ident]);
        out.extend([self.type_ident]);
        out.extend([self.equals_ident]);
        out.extend([self.match_ident]);
        out.extend([self.match_parens_ident]);
        // TODO: is there a better way to handle this?
        // while we wish to give the macro writer flexibility in their match body,
        // most match body values wont be valid, so we enforce that
        // after the macro module runs, we simply replace the body with
        // _ => ()
        // which will always be valid.
        out.extend(TokenStream::from_str("{ _ => () }").unwrap());
        // out.extend([self.match_body]);
        if let Some(semicolon) = self.semicolon_ident {
            out.extend([semicolon]);
        } else {
            // it always has to end w/ a semi-colon so we'll help the user out :P
            let punct = expect_punct(';');
            out.extend([punct]);
        }
        out
    }

    pub fn get_name(&self) -> String {
        self.statement_name.clone()
    }

    pub fn set_name(&mut self, s: &str) {
        self.statement_name = s.into();
        if let TokenTree::Ident(id) = &mut self.statement_name_ident {
            let span = id.span();
            let mut new_id = Ident::new(s, span);
            self.statement_name_ident = TokenTree::Ident(new_id);
        }
    }

    pub fn get_string_tuple_group(s: TokenStream) -> Result<Vec<String>, String> {
        let mut out = vec![];
        let mut expect_punct = false;
        let mut last_ident_str: Option<String> = None;
        for token in s {
            match token {
                TokenTree::Punct(p) => {
                    let p_char = p.as_char();
                    if let Some(last_ident) = &mut last_ident_str {
                        if p_char == ':' {
                            last_ident.push(p_char);
                        } else {
                            // otherwise, we end the last_ident string and output it:
                            match get_const(&last_ident) {
                                Some(s) => {
                                    out.push(s);
                                }
                                None => {
                                    return Err(format!("Failed to resolve value for {:?}", last_ident));
                                }
                            }
                            last_ident_str = None;
                        }
                        continue;
                    }
                    if expect_punct {
                        expect_punct = false;
                        continue;
                    }
                }
                TokenTree::Literal(s) => {
                    let mut s = s.to_string();
                    loop {
                        if s.starts_with('"') && s.ends_with('"') {
                            s.remove(0);
                            s.pop();
                        } else {
                            break
                        }
                    }
                    out.push(s);
                    expect_punct = true;
                }
                TokenTree::Ident(id) => {
                    if let Some(last_ident) = &mut last_ident_str {
                        last_ident.push_str(&id.to_string());
                    } else {
                        last_ident_str = Some(id.to_string());
                    }
                }
                x => return Err(format!("Match statements can only contain string literal values to match against. {:?} is invalid", x))
            }
        }
        Ok(out)
    }

    pub fn fill_match_statements(&mut self) -> Result<(), String> {
        let group = if let TokenTree::Group(g) = &self.match_body {
            g
        } else {
            return Err(format!("Match does not contain a match body group?"));
        };
        let mut out = vec![];
        let mut parens1: Option<Vec<String>> = None;
        let mut expect_equals = false;
        let mut expect_arrow = false;
        for token in group.stream() {
            match token {
                TokenTree::Group(g) => {
                    if g.delimiter() != Delimiter::Parenthesis {
                        return Err(format!("Match body can only contain () groups {:?} is invalid", g));
                    }
                    match parens1.take() {
                        // we have first group, so get the 2nd group and output it.
                        Some(first_part) => {
                            let a = Self::get_string_tuple_group(g.stream())?;
                            out.push((first_part, a));
                        }
                        None => {
                            // we dont have the first group yet, so get it:
                            let a = Self::get_string_tuple_group(g.stream())?;
                            parens1 = Some(a);
                            expect_equals = true;
                        }
                    }
                }
                TokenTree::Punct(p) => {
                    let p_char = p.as_char();
                    if p_char == ',' {
                        continue;
                    }
                    if expect_equals && p_char == '=' {
                        expect_equals = false;
                        expect_arrow = true;
                        continue;
                    } else if expect_arrow && p_char == '>' {
                        expect_arrow = false;
                        expect_equals = false;
                        continue;
                    }
                    return Err(format!("Unexpected punctuation while parsing match body: {:?}", p));
                }
                x => {
                    return Err(format!("Match body can only contain parentheses groups and punctuation. {:?} is invalid.", x));
                }
            }
        }
        self.match_statements = out;

        Ok(())
    }

    pub fn fill_match_against(&mut self) -> Result<(), String> {
        let group = if let TokenTree::Group(g) = &self.match_parens_ident {
            g
        } else {
            return Err(format!("Match does not contain a parentheses group?"));
        };
        let out = Self::get_string_tuple_group(group.stream())?;
        self.match_against = out;
        Ok(())
    }
}

impl Default for MatchDef {
    fn default() -> Self {
        Self {
            pub_ident: None,
            const_ident: expect_ident("fn"),
            statement_name: Default::default(),
            statement_name_ident: expect_ident("fn"),
            punct_ident: expect_ident("fn"),
            type_ident: expect_ident("fn"),
            equals_ident: expect_ident("fn"),
            match_ident: expect_ident("fn"),
            match_parens_ident: expect_ident("fn"),
            match_body: expect_ident("fn"),
            semicolon_ident: None,

            match_against: vec![],
            match_statements: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub fn_async_ident: Option<TokenTree>,
    pub fn_pub_ident: Option<TokenTree>,
    pub fn_unsafe_ident: Option<TokenTree>,
    pub fn_const_ident: Option<TokenTree>,
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
            fn_pub_ident: None,
            fn_unsafe_ident: None,
            fn_const_ident: None,
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
        if let Some(pub_ident) = self.fn_pub_ident {
            out.extend([pub_ident]);
        }
        match (self.fn_async_ident, self.fn_const_ident) {
            (None, None) => {},
            (None, Some(const_ident)) => {
                out.extend([const_ident]);
            }
            (Some(async_ident), None) => {
                out.extend([async_ident]);
            }
            (Some(_), Some(_)) => {
                panic!("Function {} is marked as both const and async", self.fn_name.to_string());
            }
        }
        if let Some(unsafe_ident) = self.fn_unsafe_ident {
            out.extend([unsafe_ident]);
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
        let params = if let TokenTree::Group(g) = &self.fn_params {
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
            // type of the param:
            // for complex types like Result<Result<A, B>, C>
            // we have to keep parsing until we reach the end of the type
            // or we reach the end of the params (eg: `fn(a: A)` )
            let mut expect_brackets = 0;
            let mut out_type: String = "".into();
            loop {
                let token = match iter.next() {
                    Some(t) => t,
                    None => { break }
                };
                match token {
                    TokenTree::Group(g) => {
                        out_type.push_str(&g.to_string());
                    }
                    TokenTree::Ident(id) => {
                        out_type.push_str(&id.to_string());
                    }
                    TokenTree::Punct(p) => {
                        let p_char = p.as_char();
                        if p_char == ',' && expect_brackets == 0 {
                            break;
                        }
                        out_type.push(p_char);
                        if p_char == '<' {
                            expect_brackets += 1;
                            continue;
                        }
                        if p_char == '>' {
                            expect_brackets -= 1;
                            if expect_brackets == 0 {
                                break;
                            }
                        }
                    }
                    TokenTree::Literal(x) => {
                        panic!("Unexpected literal {:?} while parsing function params", x);
                    }
                }
            }
            self.params.push((name, out_type));
        }
    }
    pub fn get_return_type(&self) -> String {
        let mut out: String = "".into();
        for token in &self.fn_return {
            out.push_str(&token.to_string());
        }
        out
    }
    pub fn set_func_name(&mut self, new_name: &str) {
        if let TokenTree::Ident(id) = &self.fn_name {
            let span = id.span();
            self.fn_name = TokenTree::Ident(Ident::new(new_name, span));
        } else {
            panic!("Expected fn_name to be an ident. instead found {:?}", self.fn_name);
        }
    }
    pub fn get_func_name(&self) -> String {
        if let TokenTree::Ident(id) = &self.fn_name {
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

#[derive(Debug, Clone)]
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
        if let TokenTree::Group(g) = &mut self.mod_body {
            let mut old_body = g.stream();
            let span = g.span();
            old_body.extend(add);
            let mut new_group = Group::new(Delimiter::Brace, old_body);
            new_group.set_span(span);
            self.mod_body = TokenTree::Group(new_group);
        }
    }
    pub fn get_module_name(&self) -> String {
        if let TokenTree::Ident(id) = &self.mod_name_ident {
            return id.to_string();
        } else {
            panic!("Module missing name");
        }
    }
    pub fn set_module_name(&mut self, name: &str) {
        if let TokenTree::Ident(id) = &self.mod_name_ident {
            let ident = Ident::new(name, id.span());
            self.mod_name_ident = TokenTree::Ident(ident);
        } else {
            panic!("Missing module name");
        }
    }
    pub fn contains_tokens(&self, token_stream: TokenStream) -> bool {
        let mut match_tokens = vec![];
        for token in token_stream {
            match_tokens.push(token);
        }
        let mut match_index = 0;
        let mut expect = &match_tokens[match_index];
        if let TokenTree::Group(g) = &self.mod_body {
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
    match parse_mod_def_safe(token_stream) {
        Ok(o) => o,
        Err(e) => panic!("{e}"),
    }
}

pub fn parse_mod_def_safe(token_stream: TokenStream) -> Result<ModDef, String> {
    let mut out = ModDef::default();
    let mut iter = token_stream.into_iter();
    let generic_err = "Error parsing: Unexpected end of token stream. This can only be applied to modules. Are you sure you added this macro attribute to a module?";
    let mut next = iter.next().ok_or_else(|| generic_err)?;
    let mut expect = expect_ident("pub");
    let actual_ident = assert_token_safe(&next, &expect, true)?;
    if actual_ident == "pub" {
        out.pub_ident = Some(next);
        next = iter.next().ok_or_else(|| generic_err)?;
        expect = expect_ident("mod");
        assert_token_safe(&next, &expect, false)?;
        out.mod_ident = next;
    } else if actual_ident == "mod" {
        out.mod_ident = next;
    } else {
        return Err(format!("Unexpected identifier parsing module: {:?}", next));
    }
    // we expect this to be the name of the module
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, true)?;
    out.mod_name_ident = next;
    // now we expect the mod body, so it should be a group
    expect = expect_group(Delimiter::Brace);
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.mod_body = next;
    Ok(out)
}

pub fn parse_match_def_safe(token_stream: TokenStream) -> Result<MatchDef, String> {
    let mut out = MatchDef::default();
    let mut expect = expect_ident("const");
    let mut iter = token_stream.into_iter();
    let generic_err = "Error parsing: Unexpected end of token stream. This can only be applied to match blocks. Are you sure you added this macro attribute to a match block?";
    // first keyword must be const or pub
    let mut next = iter.next().ok_or_else(|| generic_err)?;
    let ident_val = assert_token_safe(&next, &expect, true)?;
    if ident_val == "pub" {
        out.pub_ident = Some(next);
        // next one must be const then.
        next = iter.next().ok_or_else(|| generic_err)?;
        assert_token_safe(&next, &expect, false)?;
        out.const_ident = next;
    } else {
        out.const_ident = next;
    }
    // second is an ident of their 'module' name. can be any valid ident.
    next = iter.next().ok_or_else(|| generic_err)?;
    out.statement_name = assert_token_safe(&next, &expect, true)?;
    out.statement_name_ident = next;
    // must be a : punct
    expect = expect_punct(':');
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.punct_ident = next;
    // must be the () type.
    expect = expect_group(Delimiter::Parenthesis);
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, true)?;
    out.type_ident = next;
    // must be a = punct
    expect = expect_punct('=');
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.equals_ident = next;
    // must be a match ident
    expect = expect_ident("match");
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.match_ident = next;
    // must be a group
    expect = expect_group(Delimiter::Parenthesis);
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.match_parens_ident = next;
    // must be a group
    expect = expect_group(Delimiter::Brace);
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.match_body = next;
    out.fill_match_against()?;
    out.fill_match_statements()?;

    Ok(out)
}

pub fn parse_func_def_safe(token_stream: TokenStream, assert_async: bool) -> Result<FuncDef, String> {
    let mut out = FuncDef::default();
    let mut expect = expect_ident("async");
    let mut iter = token_stream.into_iter();
    let generic_err = "Error parsing: Unexpected end of token stream. This can only be applied to functions. Are you sure you added this macro attribute to a function?";
    let mut next: TokenTree;

    // loop until we hit the 'fn' identifier
    loop {
        next = iter.next().ok_or_else(|| generic_err)?;
        let actual_ident = assert_token_safe(&next, &expect, true)?;
        match actual_ident.as_str() {
            "const" => {
                out.fn_const_ident = Some(next);
            },
            "fn" => {
                out.fn_ident = next;
                break;
            },
            "async" => {
                out.fn_async_ident = Some(next);
            },
            "pub" => {
                out.fn_pub_ident = Some(next);
            },
            "unsafe" => {
                out.fn_unsafe_ident = Some(next);
            },
            x => return Err(format!("Unexpected identifier while parsing function signature '{x}'")),
        }
    }
    expect = expect_ident("fn"); // we expect next to be the name of the function
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, true)?;
    out.fn_name = next;
    expect = expect_group(Delimiter::Parenthesis);
    next = iter.next().ok_or_else(|| generic_err)?;
    assert_token_safe(&next, &expect, false)?;
    out.fn_params = next;
    next = iter.next().ok_or_else(|| generic_err)?;
    // next can either be punctuation for the return type, or the body of the function def
    match &next {
        TokenTree::Punct(p) => {
            if p.as_char() != '-' { return Err(format!("Error parsing: Expected punctuation '-', instead found {:?}", p)) }
            out.fn_return_punct.push(next);
            next = iter.next().ok_or_else(|| generic_err)?;
            if let TokenTree::Punct(p) = &next {
                if p.as_char() != '>' { return Err(format!("Error parsing: Expected punctuation '-', instead found {:?}", p)) }
            }
            out.fn_return_punct.push(next);
            // now we parse the return type.
            loop {
                next = iter.next().ok_or_else(|| generic_err)?;
                if let TokenTree::Group(g) = &next {
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
            return Err(format!("Error parsing: Expected return type for function. Instead found {:?}", next));
        }
    }

    Ok(out)
}

pub fn parse_func_def(token_stream: TokenStream, assert_async: bool) -> FuncDef {
    match parse_func_def_safe(token_stream, assert_async) {
        Ok(o) => o,
        Err(e) => panic!("{e}"),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_parse_func_defs() {
        let fdefs = [
            // is_async, is_public, is_unsafe, is_const, func def
            (false,       false,    false,     false,    "fn hello(x: String) -> String { \"a\".into() }"),
            (false,       true,     false,     false,    "pub fn hello(x: String) -> String { \"a\".into() }"),
            (true,        false,    false,     false,    "async fn hello(x: String) -> String { \"a\".into() }"),
            (false,       false,    true,      false,    "unsafe fn hello(x: String) -> String { \"a\".into() }"),
            (false,       false,    false,     true,     "const fn hello(x: String) -> String { \"a\".into() }"),
            (false,       true,     true,      true,     "pub const unsafe fn hello(x: String) -> String { \"a\".into() }"),
            (true,        true,     true,      false,    "pub async unsafe fn hello(x: String) -> String { \"a\".into() }"),
        ];
        for (is_async, is_public, is_unsafe, is_const, fdef) in fdefs {
            let stream: TokenStream = fdef.parse().unwrap();
            let mut fdef = parse_func_def_safe(stream, false).expect("Failed to parse");
            assert_eq!(fdef.fn_async_ident.is_some(), is_async);
            assert_eq!(fdef.fn_pub_ident.is_some(), is_public);
            assert_eq!(fdef.fn_unsafe_ident.is_some(), is_unsafe);
            assert_eq!(fdef.fn_const_ident.is_some(), is_const);
            assert_eq!(fdef.get_return_type(), "String");
            fdef.assert_num_params(1);
        }
    }
}
