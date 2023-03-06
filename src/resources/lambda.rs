use super::*;

pub static mut CREATED_LAMBDA: bool = false;

#[derive(Debug)]
pub struct PolicyStatement {
    pub effect: String,
    pub action: String,
    pub resource: String,
}
impl PolicyStatement {
    pub fn from_attribute_map(mut map: HashMap<String, AttributeValue>) -> Self {
        let effect = match map.remove("effect") {
            Some(e) => {
                e.assert_str("effect")
            }
            None => {
                // if no 'effect' key is provided, assume user meant allow.
                // only Denys should be explicit.
                "Allow".to_string()
            }
        };
        if effect != "Deny"  && effect != "Allow" {
            panic!("policy statement effect must be Allow or Deny");
        }
        let action = match map.remove("action") {
            Some(e) => {
                e.assert_str("action")
            }
            None => panic!("policy statement must include 'action'")
        };
        let resource = match map.remove("resource") {
            Some(e) => {
                e.assert_str("resource")
            }
            None => panic!("policy statement must include 'resource'")
        };
        PolicyStatement { effect, action, resource }
    }
}

pub struct LambdaFunction {
    pub memory: String,
    pub timeout: String,
    pub environment_variables: HashMap<String, String>,
    pub tags: HashMap<String, String>,
    pub description: String,
    pub policy_statements: Vec<PolicyStatement>,
}

impl Default for LambdaFunction {
    fn default() -> Self {
        Self {
            memory: "128".into(),
            timeout: "30".into(),
            environment_variables: Default::default(),
            tags: Default::default(),
            description: Default::default(),
            policy_statements: Default::default(),
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
                "policy_statements" => {
                    let vars = value.assert_list("policy_statements");
                    for v in vars {
                        let statement = v.assert_map("policy_statements");
                        out.policy_statements.push(PolicyStatement::from_attribute_map(statement));
                    }
                }
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
    let mut policy_str = "".to_string();
    if !lambda_conf.policy_statements.is_empty() {
        policy_str.push_str("            Policies:\n            - PolicyName: lambda_generated_policy\n              PolicyDocument:\n                  Version: '2012-10-17'\n                  Statement:\n");
        for statement in lambda_conf.policy_statements {
            policy_str.push_str(&format!("                    - Effect: {}\n", statement.effect));
            policy_str.push_str(&format!("                      Action: '{}'\n", statement.action));
            policy_str.push_str(&format!("                      Resource: '{}'\n", statement.resource));
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
{policy_str}
"
        ));
    }
}
