#[proc_macro_attribute]
pub fn hira_mod2(attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let attr = proc_macro2::TokenStream::from(attr);
    hira_lib::module_loading::hira_mod2(item, attr).into()
}
