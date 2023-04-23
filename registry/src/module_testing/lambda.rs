#[cfg(test)]
mod tests {
    use crate::modules::lambda::*;

    #[test]
    fn user_data_must_be_function() {
        let cb = |_a: &mut LambdaInput| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module { name: "".into(), is_pub: true, body: "".to_string(), append_to_body: vec![] };
        wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "This module can only be applied to a function");
    }

    #[test]
    fn validates_region() {
        let cb = |a: &mut LambdaInput| {
            a.region = "not-a-region".into();
        };
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Function { name: "".into(), is_pub: true, is_async: true, inputs: vec![], return_ty: "".into() };
        wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.starts_with("Invalid region code"));
    }
}
