use wasm_type_gen::*;

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
