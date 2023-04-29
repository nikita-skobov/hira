#[proc_macro]
pub fn hira_module_default(items: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let items = proc_macro2::TokenStream::from(items);
    hira_lib::module_loading::load_module_default(items).into()
}

#[proc_macro]
pub fn hira_modules(items: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let items = proc_macro2::TokenStream::from(items);
    hira_lib::module_loading::load_modules(items).into()
}

#[proc_macro_attribute]
pub fn hira(attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let attr = proc_macro2::TokenStream::from(attr);
    hira_lib::module_loading::run_module(item, attr).into()
}

/// This is a no-op during compilation.
/// its only necessary for wasm evaluation to know
/// which parts of the code to not compile into wasm
#[proc_macro_attribute]
pub fn dont_compile(_attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    item
}
