use super::*;

#[derive(Default)]
pub struct Route53RecordSet {
    pub record_type: String,
    pub name: String,
    pub hosted_zone_name: String,
    pub alias_target_dns_name: String,
    pub alias_target_hosted_zone_id: String,
}

impl From<AttributeValue> for Route53RecordSet {
    fn from(value: AttributeValue) -> Self {
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("Route53 record set attribute values must be a map. Instead found {:?}", value);
            }
        };
        let mut out = Route53RecordSet::default();
        for (key, val) in map {
            match key.as_str() {
                "record_type" => {
                    out.record_type = val.assert_str("record_type");
                }
                "name" => {
                    out.name = val.assert_str("name");
                }
                "hosted_zone_name" => {
                    out.hosted_zone_name = val.assert_str("hosted_zone_name");
                }
                "alias_target_dns_name" => {
                    out.alias_target_dns_name = val.assert_str("alias_target_dns_name");
                }
                "alias_target_hosted_zone_id" => {
                    out.alias_target_hosted_zone_id = val.assert_str("alias_target_hosted_zone_id");
                }
                x => panic!("Unexpected key '{x}' in route53 record set attributes"),
            }
        }
        if out.name.is_empty() {
            panic!("Route53 record must have a name. Example mysubdomain.mywebsite.com");
        }
        if out.hosted_zone_name.is_empty() {
            // try to guess hosted zone name based on the record name
            let name_components: Vec<&str> = out.name.split(".").collect();
            let second_to_last_index = name_components.len() - 2;
            let last_two = name_components.get(second_to_last_index..).expect("Invalid name for route53 record set. Must be a domain");
            out.hosted_zone_name = last_two.join(".");
        }
        if !out.hosted_zone_name.ends_with(".") {
            out.hosted_zone_name.push('.'); // hosted zone name must end in .
        }
        out
    }
}

pub fn add_route53_resource(conf: Route53RecordSet) {
    let resource_name = conf.name.replace(".", "").replace("_", "").replace("-", "");
    let Route53RecordSet { name, record_type, alias_target_dns_name, alias_target_hosted_zone_id, hosted_zone_name, .. } = conf;
    let out = format!("
    Route53Record{resource_name}:
        Type: AWS::Route53::RecordSet
        Properties:
            AliasTarget:
                DNSName: {alias_target_dns_name}
                HostedZoneId: {alias_target_hosted_zone_id}
            HostedZoneName: {hosted_zone_name}
            Comment: {name}
            Name: {name}
            Type: {record_type}\n"
    );
    unsafe {
        RESOURCES.push(out);
    }
}
