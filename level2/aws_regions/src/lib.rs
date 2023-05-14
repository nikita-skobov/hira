pub const VALID_AWS_REGIONS: &[&'static str] = &[
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "ca-central-1",
    "eu-north-1",
    "eu-west-3",
    "eu-west-2",
    "eu-west-1",
    "eu-central-1",
    "eu-south-1",
    "ap-south-1",
    "ap-northeast-1",
    "ap-northeast-2",
    "ap-northeast-3",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-southeast-3",
    "ap-east-1",
    "sa-east-1",
    "cn-north-1",
    "cn-northwest-1",
    "us-gov-east-1",
    "us-gov-west-1",
    "us-gov-secret-1",
    "us-gov-topsecret-1",
    "us-gov-topsecret-2",
    "me-south-1",
    "af-south-1",
];

pub fn is_valid_region(r: &str) -> bool {
    VALID_AWS_REGIONS.contains(&r)
}

pub fn verify_region(r: &str) -> Option<String> {
    if !is_valid_region(r) {
        Some(format!("Invalid region code {:?}\nMust be one of {:?}", r, VALID_AWS_REGIONS))
    } else {
        None
    }
}
