fn mod_macro(x) {
    x.add_code_inside("pub fn generatedfunc() {}");
    x
}
