#[hira::hira] mod _typehints {}

const VALID_AWS_REGIONS: &[&'static str] = &[
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

#[derive(Default)]
pub struct S3Input {
    /// logical name of the resource referenced in cloudformation.
    /// by default this is `S3{mod_name}`.
    /// Must be alphanumeric, and up to 255 characters.
    pub resource_name: String,
    /// physical name of the S3 bucket. Must be globally unique. By default this is
    /// set to the name of your module + hash of the module name at the end.
    /// Set this to an empty string to rely on cloudformation to create a random bucket name.
    /// Setting this field to anything other than an empty string means we will try to use the exact
    /// value you provided.
    pub bucket_name: String,
    /// internal type: stores the mod name original so we can modify it depending on user settings.
    mod_name_original: String,
    /// controls length of the hash suffix appended to the bucket name. Does not apply
    /// if bucket_name is set to an empty string. Disable adding a hash suffix by settings this value to 0.
    /// By default we set this to 8.
    pub hash_suffix_length: usize,

    /// region of the bucket. By default we set us-west-2.
    pub region: String,
}

pub type ExportType = S3Input;

impl S3Input {
    const RESOURCE_NAME_PREFIX: &'static str = "S3";

    pub fn apply_hash_to_bucket_name(&mut self, obj: &mut LibraryObj) {
        if self.bucket_name.is_empty() {
            return;
        }
        if self.hash_suffix_length == 0 {
            self.bucket_name = self.mod_name_original.clone();
            return;
        }
        let hash = obj.adler32(self.mod_name_original.as_bytes());
        let mut hash_str = format!("{:08x}", hash);
        hash_str.truncate(self.hash_suffix_length);
        self.bucket_name = format!("{}-{}", self.mod_name_original, hash_str);
    }
    pub fn new(mod_name: String, obj: &mut LibraryObj) -> Self {
        let mut out = Self::default();
        out.mod_name_original = mod_name.replace("_", "-");
        out.bucket_name = out.mod_name_original.clone();
        let resource_name = format!("{}{}", Self::RESOURCE_NAME_PREFIX, out.bucket_name);
        out.resource_name = resource_name.replace("-", "");
        out.hash_suffix_length = 8;
        out.region = "us-west-2".to_string();
        out.apply_hash_to_bucket_name(obj);
        out
    }
    pub fn is_valid(&self, obj: &mut LibraryObj) -> bool {
        if self.resource_name.len() > 255 {
            obj.compile_error(&format!("Invalid resource name {:?}\nmust be less than 255 characters", self.resource_name));
        }
        if self.resource_name.len() < 1 {
            obj.compile_error(&format!("Invalid resource name {:?}\nMust contain at least 1 character", self.resource_name));
        }
        if !self.resource_name.chars().all(|c| c.is_ascii_alphanumeric()) {
            obj.compile_error(&format!("Invalid resource name {:?}\nMust contain only alphanumeric characters [A-Za-z0-9]", self.resource_name));
        }
        if !VALID_AWS_REGIONS.contains(&self.region.as_str()) {
            obj.compile_error(&format!("Invalid region code {:?}\nMust be one of {:?}", self.region, VALID_AWS_REGIONS));
        }
        // these checks are only valid if the user didnt remove the bucket name.
        // if they made it empty, that means we let CFN generate the name.
        if !self.bucket_name.is_empty() {            
            if self.bucket_name.len() > 63 || self.bucket_name.len() < 3 {
                obj.compile_error(&format!("Invalid bucket name {:?}\nMust be between 3 and 63 characters", self.bucket_name));
            }
            let valid_char_check = |c: char| -> bool {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'
            };
            if !self.bucket_name.chars().all(valid_char_check) {
                obj.compile_error(&format!("Invalid bucket name {:?}\nMay only contain lowercase letters, numbers, dots, and dashes", self.bucket_name));
            }
            let mut chars = self.bucket_name.chars();
            let first_char = chars.next().unwrap(); // safe because we checked the length.
            let last_char = chars.last().unwrap(); // safe because we checked the length.
            if !first_char.is_ascii_alphanumeric() || !last_char.is_ascii_alphanumeric() {
                obj.compile_error(&format!("Invalid bucket name {:?}\nFirst and last character mut be either lowercase letter, or number", self.bucket_name));
            }
            if self.bucket_name.contains("..") {
                obj.compile_error(&format!("Invalid bucket name {:?}\nMay not contain two consecutive dots", self.bucket_name));
            }
        }

        obj.compiler_error_message.is_empty()
    }
    pub fn output_cfn(&self) -> String {
        let Self { resource_name, bucket_name, .. } = self;
        let bucket_name = if bucket_name.is_empty() {
            "# BucketName will be auto-generated".into()
        } else {
            format!("BucketName: {bucket_name}")
        };

        let x = format!(
r#"    {resource_name}:
        Type: 'AWS::S3::Bucket'
        Properties:
            {bucket_name}
"#);
        x
    }
}

pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut S3Input)) -> S3Input {
    let (mod_name, _mod_def, _append_to_mod_def) = match &mut obj.user_data {
        UserData::Module { name, body, append_to_body, .. } => {
            (name, body, append_to_body)
        }
        _ => {
            obj.compile_error("this module can only be used on mod definitions");
            return S3Input::default();
        }
    };
    let mut s3input = S3Input::new(mod_name.to_string(), obj);
    let bucket_name_before = s3input.bucket_name.clone();
    cb(&mut s3input);
    // only apply hash settings if the user didn't provide a bucket name.
    // if they did, then it means they want to use a specific bucket name, so we ignore our customization.
    if bucket_name_before == s3input.bucket_name {
        s3input.apply_hash_to_bucket_name(obj);
    }
    if !s3input.is_valid(obj) {
        return S3Input::default();
    }
    let cfn_resources = s3input.output_cfn();
    let region = &s3input.region;
    let deploycfncmd = format!("AWS_REGION=\"{region}\" aws --region {region} cloudformation deploy --stack-name hira-gen-stack --template-file deploy.yml --capabilities CAPABILITY_NAMED_IAM --parameter-overrides DefaultParam=hira ");

    let cfn_file = "deploy.yml";
    let deploy_file = "deploy.sh";
    let deploy = "# 3. deploy:";

    obj.append_to_line(deploy_file, deploy, deploycfncmd, "".to_string());
    obj.append_to_file_unique(cfn_file, "# 0", "AWSTemplateFormatVersion: '2010-09-09'".into());
    obj.append_to_file_unique(cfn_file, "# 0", "Parameters:".into());
    obj.append_to_file_unique(cfn_file, "# 1", format!("    DefaultParam:\n        Type: String"));
    obj.append_to_file_unique(cfn_file, "# 2", "Resources:".into());
    obj.append_to_file(cfn_file, "# 3", cfn_resources);

    // only useful for testing
    s3input
}
