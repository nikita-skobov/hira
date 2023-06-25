use hira::hira;
use aws_s3::aws_s3;
use dotenv_reader::dotenv_reader;
use ::aws_cloudfront_distribution::s3_website_distribution::s3_website_distribution;

#[hira]
pub mod myvars {
    use super::dotenv_reader;

    pub mod outputs {
        pub const ACM_ARN: &str = "this will be replaced by the value in the .env file. If not found, this string will be used as the default";
        pub const MY_DOMAIN: &str = "hiracli.com";
    }

    pub fn config(inp: &mut dotenv_reader::Input) {
        inp.dotenv_path = ".env".to_string();
    }
}

#[hira]
pub mod hiradocs_bucket {
    use super::aws_s3;
    pub mod outputs {
        pub use super::aws_s3::outputs::*;
    }
    pub fn config(inp: &mut aws_s3::Input) {
        inp.is_website = true;
    }
}

#[hira]
pub mod websitedistr {
    use super::myvars::outputs::{ACM_ARN, MY_DOMAIN};
    use super::hiradocs_bucket::outputs::LOGICAL_BUCKET_NAME;
    use super::s3_website_distribution;
    use self::s3_website_distribution::CustomDomainSettings;

    pub fn config(distrinput: &mut s3_website_distribution::Input) {
        distrinput.logical_bucket_website_url = LOGICAL_BUCKET_NAME.to_string();
        distrinput.custom_domain_settings = Some(
            CustomDomainSettings {
                acm_arn: ACM_ARN.to_string(),
                domain_name: MY_DOMAIN.to_string(),
                subdomain: Some("docs".to_string()),
                enable_route_53: false,
                ..Default::default()
            }
        );
    }
}
