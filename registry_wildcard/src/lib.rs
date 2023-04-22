use quote::{quote, format_ident};

/// lol this is a dumb thing. basically: i don't want to modify my module file
/// every time i create a new module in the modules/ directory, so this is a macro
/// that will just add the `pub mod {MODULE_NAME}` for every file it finds in the modules/ dir
#[proc_macro]
pub fn wildcard_modules(items: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut items_iter = items.into_iter();
    let path = match items_iter.next() {
        Some(proc_macro::TokenTree::Literal(l)) => {
            let mut path = l.to_string();
            while path.starts_with('"') && path.ends_with('"') {
                path.remove(0);
                path.pop();
            }
            path
        }
        _ => panic!("wildcard_modules expects first argument to be a literal string"),
    };

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(".".into());
    let module_dir = format!("{manifest_dir}{path}");
    let readdir = std::fs::read_dir(&module_dir).expect("failed to readdir while scanning for modules");
    let mut outputs = vec![];
    for entry in readdir {
        let entry = entry.expect("failed to read entry while scanning for module");
        let path = entry.path();
        let module_name = path.file_stem().expect("failed to read file stem of entry while scanning for module");
        let module_name = module_name.to_string_lossy().to_string();
        let mod_name_ident = format_ident!("{module_name}");
        outputs.push(quote! {
            pub mod #mod_name_ident;
        });
    }

    let output = quote! {
        #(#outputs)*
    };

    proc_macro::TokenStream::from(output)
}
