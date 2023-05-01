#[cfg(test)]
mod tests {
    use crate::modules::lambda::*;

    fn assert_shared_file_contains_line(data: &Vec<SharedOutputEntry>, filename: &str, line: &str) {
        let mut contained = false;
        let mut file_contents = "".to_string();
        for entry in data.iter() {
            if entry.filename == filename {
                let mut file_line = entry.line.to_string();
                if let Some(after) = &entry.after {
                    file_line.push_str(&after);
                }
                file_contents.push_str(&file_line);
                file_contents.push('\n');
                if file_line.contains(line) {
                    contained = true;
                }
            }
        }
        if !contained {
            assert_eq!(file_contents, line);
        }
    }

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
        let user_input = UserInput {
            is_self: false,
            name: "hello".to_string(),
            ty: "String".to_string(),
        };
        obj.user_data = UserData::Function { name: "".into(), is_pub: true, is_async: true, inputs: vec![user_input], return_ty: "String".into() };
        wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.starts_with("Invalid region code"));
    }

    #[test]
    fn outputs_properly_formatted_deployment_line() {
        let cb = |_: &mut LambdaInput| {};
        let mut obj = LibraryObj::new();
        let user_input = UserInput {
            is_self: false,
            name: "hello".to_string(),
            ty: "String".to_string(),
        };
        obj.user_data = UserData::Function { name: "hello".into(), is_pub: true, is_async: true, inputs: vec![user_input], return_ty: "String".into() };
        wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.is_empty());

        assert_shared_file_contains_line(&obj.shared_output_data, "deploy.sh", "AWS_REGION=\"us-west-2\" aws --region us-west-2 cloudformation deploy --stack-name hira-gen-stack --template-file deploy.yml --capabilities CAPABILITY_NAMED_IAM --parameter-overrides DefaultParam=hira ArtifactBuckethello=$artifactbucketnamehello ArtifactKeyhello=hello_$md5hello.zip");
    }
}
