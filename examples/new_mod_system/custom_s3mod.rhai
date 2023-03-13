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
    if "public_website" in HIRA_MOD_INPUT {
        access_control = "PublicRead";
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
${website(HIRA_MOD_INPUT, module_name)}
${custom_cleanup_resources(HIRA_MOD_INPUT, module_name)}
    `);

    x
}

fn website(mod_input, module_name) {
    if !("public_website" in mod_input) {
        return "";
    }
    let index_document = "index.html";
    let error_document = "error.html";
    if "index_document" in mod_input.public_website {
        index_document = mod_input.public_website.index_document;
    }
    if "error_document" in mod_input.public_website {
        error_document = mod_input.public_website.error_document;
    }

    let resource_name = `Bucket${module_name}`;
    let s3_resource_ref = "${" + resource_name + "}";
    return `
            WebsiteConfiguration:
                IndexDocument: ${index_document}
                ErrorDocument: ${error_document}
    S3BucketWebsitePolicy${module_name}:
        Type: AWS::S3::BucketPolicy
        Properties:
            Bucket: !Ref ${resource_name}
            PolicyDocument:
                Version: '2012-10-17'
                Statement:
                    - Action: 's3:GetObject'
                      Effect: Allow
                      Principal: '*'
                      Resource: !Sub 'arn:aws:s3:::${s3_resource_ref}/*'
    `;
}

fn custom_cleanup_resources(mod_input, module_name) {
    if "no_custom_cleanup" in mod_input {
        return "";
    }
    let s3_resource_name = `Bucket${module_name}`;
    let s3_resource_name = "${" + s3_resource_name + "}";
    return `
    S3DeleteLambdaRole${module_name}:
        Type: AWS::IAM::Role
        Properties:
            AssumeRolePolicyDocument:
                Version: '2012-10-17'
                Statement:
                - Effect: Allow
                  Principal:
                      Service: lambda.amazonaws.com
                  Action:
                  - sts:AssumeRole
            ManagedPolicyArns:
            - 'arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole'
            Policies:
            - PolicyName: lambda_generated_policy
              PolicyDocument:
                  Version: '2012-10-17'
                  Statement:
                    - Effect: Allow
                      Action:
                      - s3:DeleteObject
                      - s3:ListBucket
                      Resource:
                      - !Sub 'arn:aws:s3:::${s3_resource_name}/*'
                      - !Sub 'arn:aws:s3:::${s3_resource_name}'
    S3DeleteLambda${module_name}:
        Type: AWS::Lambda::Function
        Properties:
            Runtime: nodejs12.x
            Role: !GetAtt S3DeleteLambdaRole${module_name}.Arn
            Handler: index.handler
            Code:
                ZipFile: |
                    var AWS = require('aws-sdk')
                    var response = require('cfn-response')
                    const s3 = new AWS.S3({});
                    async function listObjects(bucketName) {
                        const data = await s3.listObjects({ Bucket: bucketName }).promise();
                        const objects = data.Contents;
                        for (let obj of objects) {
                            console.log(obj.Key);
                            await s3.deleteObject({ Bucket: bucketName, Key: obj.Key }).promise();
                        }
                    }
                    exports.handler = async function(event, context) {
                        let responseType = response.SUCCESS
                        if (event.RequestType == 'Delete') {
                            try {
                                await listObjects(event.ResourceProperties.BucketName);
                            } catch (err) {
                                responseType = response.FAILED
                            }
                        }
                        await response.send(event, context, responseType)
                    }
    S3CleanupResource:
        Type: Custom::cleanupbucket${module_name}
        Properties:
            ServiceToken: !GetAtt S3DeleteLambda${module_name}.Arn
            BucketName: !Ref Bucket${module_name}
    `;
}
