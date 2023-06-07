use super::*;

/// a higher level construct for creating a cloudfront distribution
/// that points to lambda function URLs, where each lambda is a separate
/// origin.
#[hira::hira]
pub mod s3_website_distribution {
    extern crate cloud_front;
    extern crate cfn_resources;

    use super::L0Core;
    use super::aws_cloudfront_distribution;
    // use self::cfn_resources::ToOptStrVal;
    pub use self::aws_cloudfront_distribution::CustomDomainSettings;
    pub use self::cloud_front::distribution::Origin;
    pub use self::cloud_front::distribution::CfnDistribution;
    pub use self::cloud_front::distribution::CustomOriginConfig;
    pub use self::cloud_front::distribution::DistributionConfig;
    pub use self::cloud_front::distribution::DefaultCacheBehavior;
    pub use self::cloud_front::distribution::CacheBehavior;
    pub use self::cloud_front::distribution::CustomOriginConfigOriginProtocolPolicyEnum;
    pub use self::cloud_front::distribution::DefaultCacheBehaviorViewerProtocolPolicyEnum;

    pub mod outputs {
        pub use super::aws_cloudfront_distribution::outputs::*;
    }

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct Input {
        /// this should be the logical id of the s3 bucket that is setup as a website.
        /// internally, we convert this to be:
        /// { "Fn::Select" : [ "2", { "Fn::Split": ["/", { "Fn::GetAtt": ["logical_bucket_website_url", "WebsiteURL"] }] } ] }
        pub logical_bucket_website_url: String,

        /// optionally provide settings to configure your distribution with a custom domain name + https cert
        pub custom_domain_settings: Option<CustomDomainSettings>,
    }

    pub fn config(inp: &mut Input, distrinput: &mut aws_cloudfront_distribution::Input, _l0core: &mut L0Core) {
        distrinput.default_origin_domain_name = aws_cloudfront_distribution::select_s3website_url(&inp.logical_bucket_website_url);
        distrinput.default_origin_protocol_policy = CustomOriginConfigOriginProtocolPolicyEnum::Httponly;
        distrinput.custom_domain_settings = inp.custom_domain_settings.clone();
    }
}