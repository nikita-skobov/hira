#[cfg(test)]
mod tests {
    use crate::modules::hira_awssqs::*;

    const HAPPY_PATH_DEFAULT: &'static str = r#"    Qmyqueue:
        Type: AWS::SQS::Queue
        Properties:
            DelaySeconds: 0
            MaximumMessageSize: 262144
            MessageRetentionPeriod: 345600
            QueueName: my_queue
            ReceiveMessageWaitTimeSeconds: 0
            SqsManagedSseEnabled: false
            VisibilityTimeout: 30"#;

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
    fn happy_path_works() {
        let cb = |_: &mut SqsInput| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Module { name: "my_queue".into(), is_pub: true, append_to_body: vec![], body: "".to_string() };
        wasm_entrypoint(&mut obj, cb as _);
        assert!(obj.compiler_error_message.is_empty());
        assert_shared_file_contains_line(&obj.shared_output_data, "deploy.yml", HAPPY_PATH_DEFAULT);
    }
}
