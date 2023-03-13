fn mod_macro(x) {
    x.add_code_before("#[cfg(hello)]");
    x.add_code_after("fn generatedfn2() {}");
    x
}
fn func_macro(x) {
    x.add_code_before("#[cfg(hello)]");
    x.add_code_after("fn generatedfn1() {}");
    x
}
