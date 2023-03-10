use super::*;

pub struct OriginAndBehavior {
    pub domain_name: String,
    pub id: String,
    pub origin_protocol_policy: String,
    pub http_port: String,
    pub https_port: String,
}

impl Default for OriginAndBehavior {
    fn default() -> Self {
        Self {
            domain_name: Default::default(),
            id: Default::default(),
            origin_protocol_policy: "http-only".into(),
            http_port: "80".into(),
            https_port: "443".into(),
        }
    }
}

impl From<AttributeValue> for OriginAndBehavior {
    fn from(value: AttributeValue) -> Self {
        let mut out = Self::default();
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("Cloudfront distribution origin/behavior value must be a map. Instead found {:?}", value);
            }
        };
        for (key, val) in map {
            match key.as_str() {
                "domain_name" => {
                    out.domain_name = val.assert_str("domain_name");
                }
                "id" => {
                    out.id = val.assert_str("id");
                }
                "origin_protocol_policy" => {
                    out.origin_protocol_policy = val.assert_str("origin_protocol_policy");
                }
                "http_port" => {
                    out.http_port = val.assert_str("http_port");
                }
                "https_port" => {
                    out.https_port = val.assert_str("https_port");
                }
                x => panic!("Unexpected key '{x}' in origin/behavior"),
            }
        }
        out
    }
}

#[derive(Default)]
pub struct CloudfrontDistribution {
    pub name: String,
    pub comment: String,
    pub acm_certificate_arn: String,
    pub aliases: Vec<String>,
    pub origins_and_behaviors: Vec<OriginAndBehavior>
}

impl From<AttributeValue> for CloudfrontDistribution {
    fn from(value: AttributeValue) -> Self {
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("Cloudfront distribution attribute values must be a map. Instead found {:?}", value);
            }
        };
        let mut out = CloudfrontDistribution::default();
        for (key, val) in map {
            match key.as_str() {
                "acm_certificate_arn" => {
                    out.acm_certificate_arn = val.assert_str("acm_certificate_arn");
                }
                "aliases" => {
                    let aliases = val.assert_list("aliases");
                    for alias in aliases {
                        out.aliases.push(alias.assert_str("alias"));
                    }
                }
                "comment" | "description" => {
                    out.comment = val.assert_str("comment");
                }
                "name" => {
                    out.name = val.assert_str("name");
                }
                "origins_and_behaviors" => {
                    let origins_and_behaviors = val.assert_list("origins_and_behaviors");
                    for oandb in origins_and_behaviors {
                        out.origins_and_behaviors.push(oandb.into());
                    }
                }
                x => panic!("Unexpected key '{x}' in cloudfront distribution attributes"),
            }
        }
        if let Some(oandb) = out.origins_and_behaviors.first_mut() {
            if oandb.id.is_empty() {
                oandb.id = "default-origin".into();
            }
        }
        out
    }
}

pub fn add_cloudfront_resource(conf: CloudfrontDistribution) {
    let resource_name = conf.name.replace("_", "");
    let cert_arn = &conf.acm_certificate_arn;
    let description = &conf.comment;
    let mut out = format!("
    CDN{resource_name}:
        Type: AWS::CloudFront::Distribution
        Properties:
            DistributionConfig:
                Enabled: 'true'\n"
    );
    if !description.is_empty() {
        out.push_str(&format!("                Comment: {description}\n"));
    }
    if !cert_arn.is_empty() {
        out.push_str(&format!("
                ViewerCertificate:
                    AcmCertificateArn: {cert_arn}
                    MinimumProtocolVersion: TLSv1.2_2021
                    SslSupportMethod: sni-only\n"
        ));
    }
    if !conf.aliases.is_empty() {
        out.push_str("                Aliases:\n");
        for alias in &conf.aliases {
            out.push_str(&format!("                - {alias}\n"));
        }
    }
    let first_origin = conf.origins_and_behaviors.first().expect("Must provide at least one origin/behavior to cloudfront distribution");
    let mut origins = vec![];
    let OriginAndBehavior { domain_name, id, http_port, https_port, origin_protocol_policy, .. } = first_origin;
    if domain_name.is_empty() {
        panic!("cloudfront distribution origin domain_name is required");
    }
    out.push_str(&format!("                DefaultCacheBehavior:
                    TargetOriginId: {id}
                    ViewerProtocolPolicy: redirect-to-https
                    CachePolicyId: 658327ea-f89d-4fab-a63d-7e88639e58f6\n"
    ));
    origins.push(format!("                - CustomOriginConfig:
                      HTTPPort: {http_port}
                      HTTPSPort: {https_port}
                      OriginProtocolPolicy: {origin_protocol_policy}
                  DomainName: {domain_name}
                  Id: {id}\n"
    ));
    // TODO: iterate over the rest of the origins
    out.push_str("                Origins:\n");
    for origin in origins {
        out.push_str(&origin);
    }
    unsafe {
        RESOURCES.push(out);
    }
}
