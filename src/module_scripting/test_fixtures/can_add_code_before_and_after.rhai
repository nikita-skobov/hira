fn mod_macro(x) {
    x.add_code_before("#[cfg(hello)]");
    x.add_code_after("fn generatedfn() {}");
    x
}
fn func_macro(x) {
    x.add_code_before("#[cfg(hello)]");
    x.add_code_after("fn generatedfn() {}");
    x
}
