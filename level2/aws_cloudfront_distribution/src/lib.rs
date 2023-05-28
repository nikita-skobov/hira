use hira_lib::level0::*;
use aws_cfn_stack::aws_cfn_stack;

#[hira::hira]
pub mod aws_cloudfront_distribution {
    extern crate cloud_front;
    extern crate cfn_resources;

    use super::L0Core;
    use super::aws_cfn_stack;
    use self::cfn_resources::StrVal;
    use self::cfn_resources::ToOptStrVal;
    pub use self::cloud_front::distribution::Origin;
    pub use self::cloud_front::distribution::CfnDistribution;
    pub use self::cloud_front::distribution::CustomOriginConfig;
    pub use self::cloud_front::distribution::DistributionConfig;
    pub use self::cloud_front::distribution::DefaultCacheBehavior;
    pub use self::cloud_front::distribution::CustomOriginConfigOriginProtocolPolicyEnum;
    pub use self::cloud_front::distribution::DefaultCacheBehaviorViewerProtocolPolicyEnum;

    pub mod outputs {
        pub const LOGICAL_DISTR_NAME: &str = "UNDEFINED";
    }

    pub struct Input {
        /// by default we create the distribution enabled and ready to use.
        /// optionally set this field to true to create the distribution
        /// but have it be disabled at first.
        pub disabled: bool,

        /// By default set to allow-all.
        pub viewer_protocol_policy: DefaultCacheBehaviorViewerProtocolPolicyEnum,

        /// the domain name of your default origin. If using an S3 bucket website
        /// this should be WebsiteUrl returned from your S3 bucket.
        /// see https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/distribution-web-values-specify.html#DownloadDistValuesDomainName
        pub default_origin_domain_name: StrVal,

        /// the policy cloudfront should use when making requests to your origin.
        /// by default we set this to http-only to optimize for creating S3 bucket websites.
        /// but you can modify this
        pub default_origin_protocol_policy: CustomOriginConfigOriginProtocolPolicyEnum,

        /// by default we only set the following fields for the default cache behavior:
        /// - cache_policy_id
        /// - viewer_protocol_policy
        /// - target_origin_id
        ///
        /// You can optionally set other settings by filling in the values of this struct.
        /// otherwise, defaults are used for all other fields.
        pub default_cache_behavior_options: DefaultCacheBehavior,

        /// by default we only set the following fields to the default origin config:
        /// - origin_protocol_policy
        ///
        /// all other fields are left default. You can optionally set those values by filling in
        /// this struct.
        pub default_origin_config_options: CustomOriginConfig,

        /// by default we only set the following fields to the distribution config:
        /// - default_cache_behavior
        /// - enabled
        /// - origins
        ///
        /// all other fields are left default. You can optionally set those values by filling in
        /// this struct.
        pub default_distribution_options: DistributionConfig,

        /// by default we only set the following fields to the default origin:
        /// - id
        /// - domain_name
        /// - custom_origin_config
        ///
        /// all other fields are left default. You can optionally set those values by filling in
        /// this struct.
        pub default_origin_options: Origin,
    }

    impl Default for Input {
        fn default() -> Self {
            Self {
                disabled: false,
                viewer_protocol_policy: DefaultCacheBehaviorViewerProtocolPolicyEnum::Allowall,
                default_cache_behavior_options: Default::default(),
                default_origin_domain_name: Default::default(),
                default_origin_protocol_policy: CustomOriginConfigOriginProtocolPolicyEnum::Httponly,
                default_origin_options: Default::default(),
                default_origin_config_options: Default::default(),
                default_distribution_options: Default::default(),
            }
        }
    }

    pub fn config(myinput: &mut Input, stackinp: &mut aws_cfn_stack::Input, l0core: &mut L0Core) {
        let user_mod_name = l0core.users_module_name();
        let enabled = !myinput.disabled;
        let default_origin_id = "origin0";
        let default_origin_config = CustomOriginConfig {
            origin_protocol_policy: myinput.default_origin_protocol_policy.clone(),
            ..myinput.default_origin_config_options.clone()
        };
        let default_origin = Origin {
            id: default_origin_id.into(),
            domain_name: myinput.default_origin_domain_name.clone().into(),
            custom_origin_config: Some(default_origin_config),
            ..myinput.default_origin_options.clone()
        };
        let distribution = CfnDistribution {
            distribution_config: DistributionConfig {
                // TODO: allow users adding origins
                origins: Some(vec![default_origin]),
                enabled,
                default_cache_behavior: DefaultCacheBehavior {
                    // caching optimized:
                    // https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/using-managed-cache-policies.html#managed-cache-caching-optimized
                    // TODO: allow customization
                    cache_policy_id: "658327ea-f89d-4fab-a63d-7e88639e58f6".to_str_val(),
                    viewer_protocol_policy: myinput.viewer_protocol_policy.clone(),
                    target_origin_id: default_origin_id.into(),
                    ..myinput.default_cache_behavior_options.clone()
                },
                ..myinput.default_distribution_options.clone()
            },
            ..Default::default()
        };

        let logical_distr_name = format!("hiragendist{user_mod_name}");
        let logical_distr_name = logical_distr_name.replace("_", "");
        let resource = aws_cfn_stack::Resource {
            name: logical_distr_name.clone(),
            properties: Box::new(distribution) as _,
        };
        stackinp.resources.push(resource);
        l0core.set_output("LOGICAL_DISTR_NAME", &logical_distr_name);
    }
}
