#[cfg(test)]
mod tests {
    use crate::modules::s3::*;

    #[test]
    fn s3_gets_renamed_properly() {
        let cb = |_a: &mut S3Input| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module { name: "my_module".into(), is_pub: true, body: "".to_string(), append_to_body: vec![] };
        let s3input = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "");
        assert_eq!(s3input.resource_name, "S3mymodule");
        // we should add a -{hash} at the end by default
        assert!(s3input.bucket_name.contains("-"));
    }

    #[test]
    fn user_can_override_bucket_name() {
        let cb = |a: &mut S3Input| {
            a.bucket_name = "something-exact".to_string();
        };
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module { name: "my_module".into(), is_pub: true, body: "".to_string(), append_to_body: vec![] };
        let s3input = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "");
        assert_eq!(s3input.resource_name, "S3mymodule");
        assert_eq!(s3input.bucket_name, "something-exact");
    }

    #[test]
    fn compile_error_on_invalid_name() {
        let cb = |a: &mut S3Input| {
            a.bucket_name = "something..exact".to_string();
        };
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module { name: "my_module".into(), is_pub: true, body: "".to_string(), append_to_body: vec![] };
        let _s3input = wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.contains("May not contain two consecutive dots"));

        let cb = |_a: &mut S3Input| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module {
            name: "my_module_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            is_pub: true,
            body: "".to_string(),
            append_to_body: vec![]
        };
        let _s3input = wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.contains("Must be between 3 and 63 characters"));
    }
}
