use hira_lib::level0::*;
use aws_cfn_stack::aws_cfn_stack;

/// this is a higher level module for creating S3 buckets easily. Some higher level
/// functionality this provides is easily setting up static website hosting.
/// In addition to creating an S3 bucket, by default we create custom cloudformation resources
/// for cleanup. That is: a lambda function will be created that will delete the contents
/// of this S3 bucket when the cloudformation stack gets deleted. This enables easy teardown.
/// See the input section to customize this behavior.
#[hira::hira]
pub mod aws_s3 {
    extern crate s3;
    extern crate lambda;
    extern crate iam;
    extern crate cfn_resources;
    
    
    use super::L0Core;
    use super::aws_cfn_stack;
    use self::cfn_resources::get_att;
    use self::cfn_resources::get_ref;
    use self::cfn_resources::create_policy_doc;
    use self::cfn_resources::StrVal;
    use self::cfn_resources::ToOptStrVal;
    use self::cfn_resources::serde_json;
    use self::cfn_resources::serde_json::Value;
    pub use self::s3::bucket::CfnBucket;
    pub use self::s3::bucket::WebsiteConfiguration;

    pub mod outputs {
        /// the logical name of the resource in cloudformation.
        /// Reference this value in other modules, for example
        /// allowing permissions to read/write from this bucket,
        /// pointing a cloudfront distribution to this bucket, etc.
        pub const LOGICAL_BUCKET_NAME: &str = "UNDEFINED";
    }

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct Input {
        /// By default, every s3 bucket gets a cleanup resource created for it.
        /// this includes:
        /// - a cloudformation custom resource
        /// - a lambda function that will perform the cleanup
        /// - a role for the lambda function that allows it to cleanup the S3 bucket.
        ///
        /// this is useful for testing, or applications that are short lived, as the cleanup resource
        /// allows you to automatically delete the S3 bucket when you delete the stack.
        /// Without a cleanup resource, deleting a stack with an S3 bucket that is not empty will fail.
        /// To disable cleanup resources, set this value to true.
        pub dont_create_cleanup_resources: bool,
        /// if enabled, we turn on website configuration for this bucket
        /// using default settings of index.html as both the error document
        /// and the index document.
        /// if set to true note that we also create a bucket policy to allow public read
        /// for every object, and we set the public access block to block_public_policy=false.
        /// if you'd like to customize this behavior, provide the website configuration
        /// in extra_bucket_settings instead, and leave this option as default.
        pub is_website: bool,
        /// this module makes no customization, instead opting for cloudformation
        /// to create the s3 bucket name for you based on the logical resource name.
        /// fill any field that you'd like to customize.
        pub extra_bucket_settings: s3::bucket::CfnBucket,
    }

    pub fn create_assume_role_policy_doc() -> Value {
        let mut map = cfn_resources::serde_json::Map::default();
        map.insert("Version".to_string(), Value::String("2012-10-17".to_string()));

        let mut principal = cfn_resources::serde_json::Map::default();
        principal.insert("Service".to_string(), Value::String("lambda.amazonaws.com".to_string()));

        let mut statements_out = vec![];
        let mut statement_obj = cfn_resources::serde_json::Map::default();
        statement_obj.insert("Effect".to_string(), Value::String("Allow".to_string()));
        statement_obj.insert("Principal".to_string(), Value::Object(principal));
        statement_obj.insert("Action".to_string(), Value::String("sts:AssumeRole".to_string()));
        statements_out.push(Value::Object(statement_obj));
        map.insert("Statement".to_string(), Value::Array(statements_out));
        Value::Object(map)
    }

    pub struct CleanupResource {
        pub lambda_logical_id: String,
        pub bucket_logical_id: String,
    }

    impl cfn_resources::CfnResource for CleanupResource {
        fn type_string(&self) -> &'static str {
            "Custom::cleanupbucket"
        }
        fn properties(&self) -> Value {
            let mut map = serde_json::Map::new();
            let service_token = get_att(&self.lambda_logical_id, "Arn");
            let bucket_name = get_ref(&self.bucket_logical_id);
            map.insert("ServiceToken".to_string(), service_token);
            map.insert("BucketName".to_string(), bucket_name);
            serde_json::Value::Object(map)
        }
    }

    pub fn config(myinput: &mut Input, stackinp: &mut aws_cfn_stack::Input, l0core: &mut L0Core) {
        let user_mod_name = l0core.users_module_name();
        let logical_bucket_name = format!("hiragenbucket{user_mod_name}");
        let logical_bucket_name = logical_bucket_name.replace("_", "");

        let website_config = WebsiteConfiguration {
            index_document: "index.html".to_str_val(),
            error_document: "index.html".to_str_val(),
            ..Default::default()
        };
        let mut bucket = s3::bucket::CfnBucket {
            ..myinput.extra_bucket_settings.clone()
        };
        
        if myinput.is_website {
            bucket.website_configuration = Some(website_config);
            if bucket.public_access_block_configuration.is_none() {
                bucket.public_access_block_configuration = Some(Default::default());
            }
            if let Some(public_block_config) = &mut bucket.public_access_block_configuration {
                public_block_config.block_public_policy = false.into();
            }
        }
        let resource = aws_cfn_stack::Resource {
            name: logical_bucket_name.clone(),
            properties: Box::new(bucket) as _,
        };
        stackinp.resources.push(resource);
        if myinput.is_website {
            let mut resource_sub = cfn_resources::serde_json::Map::new();
            // { "Fn::Sub": "arn:aws:s3:::${resource_name}/*" }
            resource_sub.insert("Fn::Sub".to_string(), cfn_resources::serde_json::Value::String(
                format!("arn:aws:s3:::${{{}}}/*", logical_bucket_name)
            ));
            let resource_sub = cfn_resources::serde_json::Value::Object(resource_sub);
            let bucket_policy = s3::bucket_policy::CfnBucketPolicy {
                bucket: StrVal::Val(get_ref(&logical_bucket_name)),
                policy_document: create_policy_doc(&[
                    ("Allow".to_string(), "s3:GetObject".to_string(), StrVal::Val(resource_sub), "*".to_str_val().unwrap()),
                ])
            };
            let logical_policy_name = format!("{logical_bucket_name}policy");
            let resource = aws_cfn_stack::Resource {
                name: logical_policy_name.clone(),
                properties: Box::new(bucket_policy) as _,
            };
            stackinp.resources.push(resource);
        }

        l0core.set_output("LOGICAL_BUCKET_NAME", &logical_bucket_name);

        // optionally setup cleanup resources:
        if myinput.dont_create_cleanup_resources {
            return;
        }
        let mut resource_sub = cfn_resources::serde_json::Map::new();
        resource_sub.insert("Fn::Sub".to_string(), cfn_resources::serde_json::Value::String(
            format!("arn:aws:s3:::${{{}}}/*", logical_bucket_name)
        ));
        let resource_sub = cfn_resources::serde_json::Value::Object(resource_sub);
        let policy = iam::role::Policy {
            policy_name: format!("hira-gen-policy-{user_mod_name}").into(),
            policy_document: create_policy_doc(&[
                (
                    "Allow".to_string(), "s3:ListBucket".to_string(),
                    StrVal::Val(get_att(&logical_bucket_name, "Arn")),
                    "".to_str_val().unwrap()
                ),
                (
                    "Allow".to_string(), "s3:DeleteObject".to_string(),
                    StrVal::Val(resource_sub),
                    "".to_str_val().unwrap()
                )
            ]),
        };
        let role_name = format!("hiragenrole{user_mod_name}");
        let logical_role_name = role_name.replace("_", "");
        let role = iam::role::CfnRole {
            description: Some(format!("auto generated cleanup resource for {user_mod_name}").into()),
            assume_role_policy_document: create_assume_role_policy_doc(),
            policies: Some(vec![policy]),
            ..Default::default()
        };
        let logical_fn_name = format!("hiragencleanupfunction{user_mod_name}");
        let logical_fn_name = logical_fn_name.replace("_", "");
        let cleanup_function = lambda::function::CfnFunction {
            runtime: lambda::function::FunctionRuntimeEnum::Nodejs16x.into(),
            handler: "index.handler".to_str_val(),
            role: get_att(&logical_role_name, "Arn").into(),
            code: lambda::function::Code {
                zip_file: r#"
                var AWS = require('aws-sdk')
                var response = require('cfn-response')
                const s3 = new AWS.S3({});
                async function listObjects(bucketName) {
                    const data = await s3.listObjects({ Bucket: bucketName }).promise();
                    const objects = data.Contents;
                    for (let obj of objects) {
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
                "#.to_str_val(),
                ..Default::default()
            },
            ..Default::default()
        };

        let cleanup = CleanupResource {
            lambda_logical_id: logical_fn_name.clone(),
            bucket_logical_id: logical_bucket_name.clone(),
        };
        let logical_cleanup_resource_name = format!("hiragencustomcleanup{user_mod_name}");
        let logical_cleanup_resource_name = logical_cleanup_resource_name.replace("_", "");
        let cleanup_resource = aws_cfn_stack::Resource {
            name: logical_cleanup_resource_name.into(),
            properties: Box::new(cleanup) as _,
        };
        let function_resource = aws_cfn_stack::Resource {
            name: logical_fn_name,
            properties: Box::new(cleanup_function) as _,
        };
        let role_resource = aws_cfn_stack::Resource {
            name: logical_role_name,
            properties: Box::new(role) as _,
        };
        stackinp.resources.push(role_resource);
        stackinp.resources.push(function_resource);
        stackinp.resources.push(cleanup_resource);
    }
}
