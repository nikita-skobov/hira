fn mod_macro(x) {
    let module_name = x.get_name();
    let bucket_name = module_name;
    module_name.replace("_", "");
    let access_control = "Private";
    if HIRA_MOD_INPUT.contains("bucket_name") {
        bucket_name = HIRA_MOD_INPUT.bucket_name;
    }
    if HIRA_MOD_INPUT.contains("access_control") {
        access_control = HIRA_MOD_INPUT.access_control;
    }

    x.add_code_after(`
    // TODO: save the client somehow. dont re-create for each request...
    pub async fn make_s3_client() -> aws_sdk_s3::Client {
        let region = "${HIRA_DEPLOY_REGION}";
        let region_provider = aws_config::meta::region::RegionProviderChain::default_provider().or_else(region);
        let sdk_config = aws_config::from_env().region(region_provider).load().await;
        aws_sdk_s3::Client::new(&sdk_config)
    }
    `);

    x.add_code_inside("use super::make_s3_client;");
    x.add_code_inside(`
    pub async fn put_object_inner(
        client: &aws_sdk_s3::Client,
        key: &str,
        data: Vec<u8>,
    ) -> Result<(), aws_sdk_s3::Error> {
        self::put_object_builder(client, key, data).send().await?;
        Ok(())
    }
    pub fn put_object_builder(client: &aws_sdk_s3::Client, key: &str, data: Vec<u8>) -> aws_sdk_s3::client::fluent_builders::PutObject {
        let b = aws_sdk_s3::types::ByteStream::from(data);
        client
            .put_object()
            .bucket("${bucket_name}")
            .key(key)
            .body(b)
    }
    pub async fn put_object(key: &str, data: Vec<u8>) -> Result<(), aws_sdk_s3::Error> {
        let client = make_s3_client().await;
        self::put_object_inner(&client, key, data).await
    }
    `);

    if x.contains_tokens("pub async fn _init()") {
        x.add_code_after(`
             #[cfg(${module_name})]
             #[tokio::main]
             async fn main() -> Result<(), ()> {
                 let _ = ${module_name}::_init().await;
                 Ok(())
             }
        `);
        add_post_build_command(`AWS_REGION="${HIRA_DEPLOY_REGION}" RUSTFLAGS="--cfg ${module_name}" cargo run --target-dir hira/cross-target-${module_name}`);
    }

    add_to_cfn(`
    Bucket${module_name}:
        Type: AWS::S3::Bucket
        Properties:
            AccessControl: ${access_control}
            BucketName: ${bucket_name}
    `);

    x
}
