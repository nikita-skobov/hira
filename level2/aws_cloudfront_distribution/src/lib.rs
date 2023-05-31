use hira_lib::level0::*;
use aws_cfn_stack::aws_cfn_stack;

/// a higher level construct for creating a cloudfront distribution
/// that points to lambda function URLs, where each lambda is a separate
/// origin.
#[hira::hira]
pub mod lambda_url_distribution {
    extern crate cloud_front;
    extern crate cfn_resources;

    
    use super::L0Core;
    use super::aws_cloudfront_distribution;
    use self::cfn_resources::ToOptStrVal;

    pub use self::aws_cloudfront_distribution::CustomDomainSettings;
    pub use self::cloud_front::distribution::Origin;
    pub use self::cloud_front::distribution::CfnDistribution;
    pub use self::cloud_front::distribution::CustomOriginConfig;
    pub use self::cloud_front::distribution::DistributionConfig;
    pub use self::cloud_front::distribution::DefaultCacheBehavior;
    pub use self::cloud_front::distribution::CacheBehavior;
    pub use self::cloud_front::distribution::CustomOriginConfigOriginProtocolPolicyEnum;
    pub use self::cloud_front::distribution::DefaultCacheBehaviorViewerProtocolPolicyEnum;

    /// represents one origin in your distribution.
    /// path is the URL path that will map to your lambda function.
    #[derive(Default)]
    pub struct LambdaApiEndpoint {
        pub path: String,
        /// The logical id of the lambda function URL that you'd like to point to.
        /// internally, we reference this logical id in order to retrieve the actual function URL.
        pub function_url_id: String,
    }

    #[derive(Default)]
    pub struct Input {
        /// at least one of your endpoints must have path = "/".
        /// this represents the default endpoint.
        /// all endpoints paths must be unique.
        pub endpoints: Vec<LambdaApiEndpoint>,

        /// optionally provide settings to configure your distribution with a custom domain name + https cert
        pub custom_domain_settings: Option<CustomDomainSettings>,
    }

    pub fn config(inp: &mut Input, distrinput: &mut aws_cloudfront_distribution::Input, l0core: &mut L0Core) {
        let mut default = None;
        let mut other_endpoints: Vec<LambdaApiEndpoint> = vec![];
        for endpoint in inp.endpoints.drain(..) {
            if endpoint.path == "/" {
                default = Some(endpoint);
            } else {
                if other_endpoints.iter().any(|x| x.path == endpoint.path) {
                    l0core.compiler_error(&format!("Lambda API distribution received duplicate endpoint path {}. All paths in a distribution must be unique", endpoint.path));
                    return;
                }
                other_endpoints.push(endpoint);
            }
        }
        let default = if let Some(d) = default {
            d
        } else {
            l0core.compiler_error("Lambda API distribution missing a default endpoint. Must provide an endpoint where path = '/'");
            return;
        };

        distrinput.default_origin_domain_name = aws_cloudfront_distribution::select_function_url(&default.function_url_id);
        distrinput.default_origin_protocol_policy = CustomOriginConfigOriginProtocolPolicyEnum::Httpsonly;
        distrinput.custom_domain_settings = inp.custom_domain_settings.clone();

        let mut extra_origins = vec![];
        for (i, endpoint) in other_endpoints.iter().enumerate() {
            let mut origin = Origin::default();
            let mut behavior = CacheBehavior::default();
            origin.id = format!("extraorigin{i}").into();
            origin.domain_name = aws_cloudfront_distribution::select_function_url(&endpoint.function_url_id);
            origin.custom_origin_config = Some(CustomOriginConfig {
                origin_protocol_policy: CustomOriginConfigOriginProtocolPolicyEnum::Httpsonly,
                // TODO: would this need any customizability for lambda functions?
                ..Default::default()
            });
            behavior.path_pattern = endpoint.path.clone().into();
            behavior.target_origin_id = origin.id.clone();
            behavior.cache_policy_id = "658327ea-f89d-4fab-a63d-7e88639e58f6".to_str_val();
            // TODO: behavior customizability?
            extra_origins.push((origin, behavior));
        }
        distrinput.extra_origins = extra_origins;
    }
}


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


    #[derive(Default)]
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

#[hira::hira]
pub mod aws_cloudfront_distribution {
    extern crate cloud_front;
    extern crate route53;
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
    pub use self::cloud_front::distribution::CacheBehavior;
    pub use self::cloud_front::distribution::CustomOriginConfigOriginProtocolPolicyEnum;
    pub use self::cloud_front::distribution::DefaultCacheBehaviorViewerProtocolPolicyEnum;
    pub use self::cloud_front::distribution::ViewerCertificateSslSupportMethodEnum;
    pub use self::cloud_front::distribution::ViewerCertificateMinimumProtocolVersionEnum;
    pub use self::cloud_front::distribution::ViewerCertificate;

    pub mod outputs {
        pub const LOGICAL_DISTR_NAME: &str = "UNDEFINED";
    }

    /// a lambda function url resource can return an attribute "FunctionUrl"
    /// but this attribute has `https://` in front of it. This makes it unsuitable
    /// to plug directly as a domain name into cloudfront, as cloudfront expects it without the protocol.
    /// This function provides a convenient wrapper that basically creates
    /// { "Fn::Select" : [ "2", { "Fn::Split": ["/", { "Fn::GetAtt": ["logicalId", "FunctionUrl"] }] } ] }
    /// You only need to provide the logical id of the resource
    pub fn select_function_url(logical_id: &str) -> cfn_resources::StrVal {
        let func_url = cfn_resources::get_att(logical_id, "FunctionUrl");
        let select_domain = cfn_resources::select_split(2, "/", func_url);
        cfn_resources::StrVal::Val(select_domain)
    }

    /// same as `select_function_url` but for s3 websites we use WebsiteURL instead of FunctionUrl
    pub fn select_s3website_url(logical_id: &str) -> cfn_resources::StrVal {
        let website_url = cfn_resources::get_att(logical_id, "WebsiteURL");
        let select_domain = cfn_resources::select_split(2, "/", website_url);
        cfn_resources::StrVal::Val(select_domain)
    }

    #[derive(Clone)]
    pub struct CustomDomainSettings {
        /// required. will error if not provided.
        pub acm_arn: String,
        /// required. will error if not provided.
        /// Note: this should not end with a .
        /// for example if your domain is mywebsite.com
        /// you should provide it exactly as "mywebsite.com"
        /// If you wish this distribution to be setup as a subdomain, you should
        /// still provide domain_name = "mywebsite.com", and then
        /// optionally set the subdomain field to Some("mysubdomain").
        pub domain_name: String,
        pub subdomain: Option<String>,
        /// by default we set this to sni-only
        pub ssl_support_method: ViewerCertificateSslSupportMethodEnum,
        /// by default we set this to TLSv1.2_2021
        pub minimum_protocol_version: ViewerCertificateMinimumProtocolVersionEnum,
        /// by default this is true, and a route53 record will be created
        /// that points from your domain_name to this cloudfront distribution.
        /// optionally set it to false if you need to customize your route53 record
        pub enable_route_53: bool,
    }

    impl Default for CustomDomainSettings {
        fn default() -> Self {
            Self {
                acm_arn: Default::default(),
                domain_name: Default::default(),
                subdomain: Default::default(),
                ssl_support_method: ViewerCertificateSslSupportMethodEnum::Snionly,
                minimum_protocol_version: ViewerCertificateMinimumProtocolVersionEnum::Tlsv122021,
                enable_route_53: true,
            }
        }
    }

    pub struct Input {
        /// by default we create the distribution enabled and ready to use.
        /// optionally set this field to true to create the distribution
        /// but have it be disabled at first.
        pub disabled: bool,

        /// By default set to allow-all.
        pub viewer_protocol_policy: DefaultCacheBehaviorViewerProtocolPolicyEnum,

        /// by default this is left empty and that means your cloudfront distribution is
        /// created with the default cloudfront domain name (eg something like: d111111abcdef8.cloudfront.net)
        /// If provided, we configure this distribution with
        /// - aliases pointing to the domain name
        /// - viewer certificate settings using the provided ACM Arn.
        ///
        /// Optionally you can also enable route53 which will create a route53 record for your domain
        /// to point to this distribution.
        pub custom_domain_settings: Option<CustomDomainSettings>,

        /// the domain name of your default origin. If using an S3 bucket website
        /// this should be WebsiteUrl returned from your S3 bucket.
        /// see https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/distribution-web-values-specify.html#DownloadDistValuesDomainName
        pub default_origin_domain_name: StrVal,

        /// the policy cloudfront should use when making requests to your origin.
        /// by default we set this to http-only to optimize for creating S3 bucket websites.
        /// but you can modify this
        pub default_origin_protocol_policy: CustomOriginConfigOriginProtocolPolicyEnum,

        /// optionally provide extra origins. Each origin consists of a pair
        /// of an Origin as well as a CacheBehavior that corresponds to that origin.
        pub extra_origins: Vec<(Origin, CacheBehavior)>,

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
        /// - viewer_certificate
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
                extra_origins: Default::default(),
                custom_domain_settings: Default::default(),
            }
        }
    }

    pub fn config(myinput: &mut Input, stackinp: &mut aws_cfn_stack::Input, l0core: &mut L0Core) {
        let user_mod_name = l0core.users_module_name();
        let logical_distr_name = format!("hiragendist{user_mod_name}");
        let logical_distr_name = logical_distr_name.replace("_", "");
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
        let (viewer_certificate, alias_config, route53_resource) = if let Some(settings) = &myinput.custom_domain_settings {
            if settings.acm_arn.is_empty() {
                l0core.compiler_error(&format!("Provided custom_domain_settings, but acm_arn field is empty. This is required."));
                return;
            }
            if settings.domain_name.is_empty() {
                l0core.compiler_error(&format!("Provided custom_domain_settings, but domain_name field is empty. This is required."));
                return;
            }
            let alias = match &settings.subdomain {
                Some(a) => {
                    if a.ends_with(".") {
                        l0core.compiler_error(&format!("Provided custom_domain_settings.subdomain '{}' ends with . This is invalid. must not end in a . as that is assumed", a));
                        return; 
                    }
                    format!("{}.{}", a, settings.domain_name)
                },
                None => settings.domain_name.clone(),
            };
            let route_53_resource = if settings.enable_route_53 {
                let record_set = route53::record_set::CfnRecordSet {
                    alias_target: route53::record_set::AliasTarget {
                        dnsname: cfn_resources::get_att(&logical_distr_name, "DomainName").into(),
                        // this is what you need to use when pointing route53 to cloudfront:
                        // https://docs.aws.amazon.com/AWSCloudFormation/latest/UserGuide/aws-properties-route53-aliastarget.html#cfn-route53-aliastarget-hostedzoneid
                        hosted_zone_id: "Z2FDTNDATAQYW2".into(),
                        ..Default::default()
                    }.into(),
                    hosted_zone_name: format!("{}.", settings.domain_name).to_str_val(),
                    comment: format!("{}", settings.domain_name).to_str_val(),
                    name: alias.clone().to_str_val().unwrap(),
                    ..Default::default()
                };
                let logical_r53_resource_name = format!("hiragenr53recort{user_mod_name}");
                let logical_r53_resource_name = logical_r53_resource_name.replace("_", "");
                let resource = aws_cfn_stack::Resource {
                    name: logical_r53_resource_name.clone(),
                    properties: Box::new(record_set) as _,
                };
                Some(resource)
            } else {
                None
            };
            let cert = ViewerCertificate {
                acm_certificate_arn: Some(settings.acm_arn.clone().into()),
                ssl_support_method: Some(settings.ssl_support_method.clone()),
                minimum_protocol_version: Some(settings.minimum_protocol_version.clone()),
                ..Default::default()
            };
            let alias_config: Option<Vec<String>> = Some(vec![alias]);
            (Some(cert), alias_config, route_53_resource)
        } else {
            (None, None, None)
        };
        let mut distribution = CfnDistribution {
            distribution_config: DistributionConfig {
                origins: Some(vec![default_origin]),
                enabled,
                viewer_certificate,
                aliases: alias_config,
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

        let mut used_origin_ids = vec![default_origin_id.to_string()];
        for (origin, behavior) in myinput.extra_origins.drain(..) {
            if let StrVal::String(s) = &origin.id {
                if used_origin_ids.contains(s) {
                    l0core.compiler_error(&format!("Origin ID '{s}' already exists in this distribution. All origin IDs must be unique."));
                    return;
                }
                used_origin_ids.push(s.to_string());
            }
            if let Some(origins) = &mut distribution.distribution_config.origins {
                origins.push(origin);
            }
            if distribution.distribution_config.cache_behaviors.is_none() {
                distribution.distribution_config.cache_behaviors = Some(vec![]);
            }
            if let Some(behaviors) = &mut distribution.distribution_config.cache_behaviors {
                behaviors.push(behavior);
            }
        }

        let resource = aws_cfn_stack::Resource {
            name: logical_distr_name.clone(),
            properties: Box::new(distribution) as _,
        };
        stackinp.resources.push(resource);
        if let Some(route53resource) = route53_resource {
            stackinp.resources.push(route53resource);
        }
        l0core.set_output("LOGICAL_DISTR_NAME", &logical_distr_name);
    }
}
