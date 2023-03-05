use super::*;

pub struct LambdaFunction {
    pub memory: String,
    pub timeout: String,
    pub environment_variables: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    pub description: String,
}

impl Default for LambdaFunction {
    fn default() -> Self {
        Self {
            memory: "128".into(),
            timeout: "30".into(),
            environment_variables: Default::default(),
            tags: Default::default(),
            description: Default::default()
        }
    }
}

impl From<AttributeValue> for LambdaFunction {
    fn from(value: AttributeValue) -> Self {
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("Lambda function attribute values must be a map. Instead found {:?}", value);
            }
        };

        let mut out = LambdaFunction::default();
        for (key, value) in map {
            match key.as_str() {
                "memory" => {
                    out.memory = value.assert_str("memory");
                },
                "timeout" => {
                    out.timeout = value.assert_str("timeout");
                },
                "environment_variables" => {
                    let vars = value.assert_map("environment_variables");
                    for (name, map_value) in vars {
                        let v = map_value.assert_str(&name);
                        out.environment_variables.insert(name, v);
                    }
                },
                "tags" => {
                    let vars = value.assert_map("tags");
                    for (name, map_value) in vars {
                        let v = map_value.assert_str(&name);
                        out.tags.insert(name, v);
                    }
                },
                "description" => {
                    out.description = value.assert_str("description");
                },
                _ => {
                    panic!("Unknown property in lambda function attribute {:?}", key);
                }
            }
        }
        out
    }
}

pub fn add_lambda_resource<S: AsRef<str>>(bucket_name: S, func_name: S, lambda_conf: LambdaFunction) {
    let func_name = func_name.as_ref();
    // lambda resources can only be alphanumeric
    let func_name_resource = func_name.replace("_", "");
    let bucket_name = bucket_name.as_ref();
    let memory = &lambda_conf.memory;
    let timeout = &lambda_conf.timeout;
    let mut environment_variables = "".to_string();
    let mut tags = "".to_string();
    if !lambda_conf.environment_variables.is_empty() {
        environment_variables.push_str("            Environment:\n                Variables:\n");
        for (key, val) in lambda_conf.environment_variables.iter() {
            environment_variables.push_str(&format!("                    {}: {}\n", key, val));
        }
    }
    if !lambda_conf.tags.is_empty() {
        tags.push_str("            Tags:\n");
        for (key, val) in lambda_conf.tags.iter() {
            tags.push_str(&format!("            - Key: {}\n              Value: {}\n", key, val));
        }
    }
    unsafe {
        RESOURCES.push(format!("
    Lambda{func_name_resource}:
        Type: 'AWS::Lambda::Function'
        Properties:
            FunctionName: {func_name}
            Runtime: provided.al2
            Code:
                S3Bucket: {bucket_name}
                S3Key: {func_name}.zip
            Handler: index.handler
{tags}
            MemorySize: {memory}
            Timeout: {timeout}
{environment_variables}
            Architectures:
            - arm64
            Role: !GetAtt LambdaExecutionRole{func_name_resource}.Arn
    LambdaExecutionRole{func_name_resource}:
        Type: AWS::IAM::Role
        Properties:
            AssumeRolePolicyDocument:
                Version: '2012-10-17'
                Statement:
                    - Effect: Allow
                      Principal:
                          Service: lambda.amazonaws.com
                      Action:
                          - sts:AssumeRole
            ManagedPolicyArns:
            - 'arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole'
"
        ));
    }
}
