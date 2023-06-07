use super::*;

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

    pub mod outputs {
        pub use super::aws_cloudfront_distribution::outputs::*;
    }

    /// represents one origin in your distribution.
    /// path is the URL path that will map to your lambda function.
    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct LambdaApiEndpoint {
        pub path: String,
        /// The logical id of the lambda function URL that you'd like to point to.
        /// internally, we reference this logical id in order to retrieve the actual function URL.
        pub function_url_id: String,
    }

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
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
