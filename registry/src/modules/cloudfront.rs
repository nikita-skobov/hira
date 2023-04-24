#[hira::hira] mod _typehints {}

pub type ExportType = CloudfrontInput;

#[derive(Default)]
pub struct CloudfrontInput {
    /// internal type used for testing.
    pub num_distributions: usize,
    /// ACM certificate ARN. must be a valid certificate for the domain name you specified.
    pub acm_arn: String,
    /// region to deploy cloudfront to
    pub region: String,
}

impl CloudfrontInput {
    pub fn new() -> Self {
        let mut out = Self::default();
        out.region = "us-west-2".to_string();
        out
    }
}

struct DistributionConfig {
    pub origins: Vec<OriginConfig>,
}

#[derive(Default)]
struct OriginConfig {
    pub origin_id: String,
    pub path_pattern: String,
    pub viewer_protocol_policy: String,
    pub allowed_methods: String,
    pub cache_policy_id: String,
    pub compress: bool,

    // specific to the origin:
    pub origin_protocol_policy: String,
    pub origin_domain_name: String,
    pub origin_base_path: String,
}

impl OriginConfig {
    pub fn new(s: String, index: usize, domain_name: &str) -> Self {
        let mut out = Self::default();
        out.path_pattern = s.to_string();
        out.origin_id = format!("origin{index}");
        out.allowed_methods = r#"[GET, HEAD]"#.to_string();
        out.viewer_protocol_policy = "redirect-to-https".to_string();
        out.cache_policy_id = "658327ea-f89d-4fab-a63d-7e88639e58f6".to_string();
        out.compress = false;
        out.origin_protocol_policy = "https-only".to_string();
        out.origin_domain_name = domain_name.to_string();
        out
    }
}

fn cfn_cache_behavior(
    origin_config: &OriginConfig
) -> String {
    let OriginConfig {
        origin_id,
        path_pattern,
        viewer_protocol_policy,
        allowed_methods,
        cache_policy_id,
        compress, ..
    } = origin_config;
    let (key_name, path_pattern) = if path_pattern == "*" {
        ("DefaultCacheBehavior", "# no path pattern for default".to_string())
    } else {
        ("CacheBehavior", format!("PathPattern: {path_pattern}"))
    };
    format!(
r#"{key_name}:
                    TargetOriginId: {origin_id}
                    {path_pattern}
                    ViewerProtocolPolicy: {viewer_protocol_policy}
                    AllowedMethods: {allowed_methods}
                    CachePolicyId: {cache_policy_id}
                    Compress: {compress}
"#
    )
}

fn cfn_origin(origin_config: &OriginConfig) -> String {
    let OriginConfig {
        origin_id,
        origin_protocol_policy,
        origin_domain_name,
        origin_base_path,
        ..
    } = origin_config;
    let origin_path = if origin_base_path.is_empty() {
        "# no origin base path".to_string()
    } else {
        format!("OriginPath: {origin_base_path}")
    };
    let x = format!(
r#"                - Id: {origin_id}
                  DomainName: {origin_domain_name}
                  {origin_path}
                  CustomOriginConfig:
                      OriginProtocolPolicy: {origin_protocol_policy}"#);
    x
}

fn cfn_cache_behaviors(origins: &[OriginConfig]) -> String {
    if origins.is_empty() {
        return "# no cache behaviors because only 1 origin".to_string();
    }
    let mut x = "CacheBehaviors:".to_string();
    for other in origins {
        x.push('\n');
        x.push_str(&cfn_cache_behavior(other));
    }
    x
}

fn cfn_origins(default_origin: &OriginConfig, rest_of_origins: &[OriginConfig]) -> String {
    let mut x = "Origins:\n".to_string();
    x.push_str(&cfn_origin(default_origin));
    for other in rest_of_origins {
        x.push('\n');
        x.push_str(&cfn_origin(other));
    }
    x
}

fn cfn_resource(
    resource_name: &str,
    host: &str,
    cert_arn: &str,
    default_origin: OriginConfig,
    other_origins: &Vec<OriginConfig>,
) -> String {
    let default_cache_behavior = cfn_cache_behavior(&default_origin);
    let origins = cfn_origins(&default_origin, other_origins);
    let cache_behaviors = cfn_cache_behaviors(other_origins);
        let x = format!(
r#"    {resource_name}:
        Type: 'AWS::CloudFront::Distribution'
        Properties:
            DistributionConfig:
                Enabled: true
                ViewerCertificate:
                    AcmCertificateArn: {cert_arn}
                    MinimumProtocolVersion: TLSv1.2_2021
                    SslSupportMethod: sni-only
                Aliases:
                - {host}
                {default_cache_behavior}
                {origins}
                {cache_behaviors}"#);
    x
}

pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut CloudfrontInput)) -> CloudfrontInput {
    let (expr, arms) = match &obj.user_data {
        UserData::Match { expr, arms, .. } => {
            (expr, arms)
        }
        _ => {
            obj.compile_error("this module can only be used on match statements. Make sure your match statement looks something like\nconst _: _ = match \"something\" { ... };");
            return CloudfrontInput::default();
        }
    };
    let mut input = CloudfrontInput::new();
    cb(&mut input);

    let default = CloudfrontInput::default();
    if expr.len() != 2 {
        obj.compile_error("this module expects the match statement expression to be a tuple of 2 strings");
        return default;
    }
    let [domain, path] = [&expr[0], &expr[1]];
    if path != "path" {
        obj.compile_error("second tuple element must be string literal \"path\"");
        return default;
    }

    let mut distributions: std::collections::HashMap<String, DistributionConfig> = Default::default();
    let default_path_pattern = "*".to_string();

    for arm in arms {
        let (pattern, expr) = (&arm.pattern, &arm.expr);
        let (domain_pattern, path_pattern) = match pattern.get(0..2) {
            Some(got) => (&got[0], &got[1]),
            None => continue,
        };
        let path_pattern_str = match path_pattern {
            None => default_path_pattern.clone(),
            Some(s) => s.to_string(),
        };
        let domain_str = match domain_pattern {
            None => domain,
            Some(s) => s,
        };
        match distributions.get_mut(domain_str) {
            Some(existing) => {
                if existing.origins.iter().any(|x| x.path_pattern == path_pattern_str) {
                    obj.compile_error(&format!("Found duplicate path pattern for distribution {}", domain_str));
                    return default;
                }
                let index = existing.origins.len();
                let config = OriginConfig::new(path_pattern_str, index, expr);
                existing.origins.push(config);
            }
            None => {
                let config = OriginConfig::new(path_pattern_str, 0, expr);
                distributions.insert(domain_str.clone(), DistributionConfig {
                    origins: vec![config],
                });
            }
        }
    }

    let mut cfn_resources = vec![];

    let mut i = 1;
    for (domain, distr) in distributions.iter_mut() {
        let (default_origin, other_origins) = if let Some(default_origin_index) = distr.origins.iter().position(|x| x.path_pattern == default_path_pattern) {
            let default_origin = distr.origins.remove(default_origin_index);
            (default_origin, std::mem::take(&mut distr.origins))
        } else {
            obj.compile_error(&format!("Distribution for {domain} is missing a default path '*'. Ensure one of your match arms has a wildcard '_' for the path component"));
            return default;
        };
        let resource_name = format!("CDN{i}");
        i += 1;
        let resource = cfn_resource(&resource_name, &domain, &input.acm_arn, default_origin, &other_origins);
        cfn_resources.push(resource);
    }
    input.num_distributions = distributions.len();


    let region = &input.region;
    let deploycfncmd = format!("AWS_REGION=\"{region}\" aws --region {region} cloudformation deploy --stack-name hira-gen-stack --template-file deploy.yml --capabilities CAPABILITY_NAMED_IAM --parameter-overrides DefaultParam=hira ");

    let deploy_file = "deploy.sh";
    let cfn_file = "deploy.yml";
    // let pre_build = "# 0. pre-build:";
    // let build = "# 1. build:";
    // let package = "# 2. package:";
    let deploy = "# 3. deploy:";

    obj.append_to_line(deploy_file, deploy, deploycfncmd, "".to_string());
    obj.append_to_file_unique(cfn_file, "# 0", "AWSTemplateFormatVersion: '2010-09-09'".into());
    obj.append_to_file_unique(cfn_file, "# 0", "Parameters:".into());
    obj.append_to_file_unique(cfn_file, "# 1", format!("    DefaultParam:\n        Type: String"));
    obj.append_to_file_unique(cfn_file, "# 2", "Resources:".into());
    for resource in cfn_resources {
        obj.append_to_file(cfn_file, "# 3", resource);
    }

    input
}
