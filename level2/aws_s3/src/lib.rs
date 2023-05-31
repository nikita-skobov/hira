use hira_lib::level0::*;
use aws_cfn_stack::aws_cfn_stack;

#[hira::hira]
pub mod aws_s3 {
    extern crate s3;
    extern crate cfn_resources;
    
    use super::L0Core;
    use super::aws_cfn_stack;
    use self::cfn_resources::get_ref;
    use self::cfn_resources::create_policy_doc;
    use self::cfn_resources::StrVal;
    use self::cfn_resources::ToOptStrVal;
    pub use self::s3::bucket::CfnBucket;
    pub use self::s3::bucket::WebsiteConfiguration;

    pub mod outputs {
        pub const LOGICAL_BUCKET_NAME: &str = "UNDEFINED";
    }

    #[derive(Default)]
    pub struct Input {
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
    }
}
