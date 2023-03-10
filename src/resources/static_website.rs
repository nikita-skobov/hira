use super::*;

#[derive(Default)]
pub struct StaticWebsite {
    pub url: String,
    pub acm_arn: String,
}

impl From<AttributeValue> for StaticWebsite {
    fn from(value: AttributeValue) -> Self {
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("Static website attribute values must be a map. Instead found {:?}", value);
            }
        };
        let mut out = StaticWebsite::default();
        for (key, val) in map {
            match key.as_str() {
                "url" => {
                    out.url = val.assert_str("url");
                }
                "acm_arn" => {
                    out.acm_arn = val.assert_str("acm_arn");
                }
                x => panic!("Unexpected key '{x}' in static website attributes"),
            }
        }
        if out.url.is_empty() {
            panic!("Must provide URL to static website attributes");
        }
        if out.acm_arn.is_empty() {
            panic!("Must provide ACM certificate ARN to static website attributes");
        }
        out
    }
}

