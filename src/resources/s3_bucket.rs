use super::*;

pub static mut CREATED_S3: bool = false;

pub struct PublicWebsite {
    pub index_document: String,
    pub error_document: String,
}

impl Default for PublicWebsite {
    fn default() -> Self {
        Self {
            index_document: "index.html".into(),
            error_document: "error.html".into()
        }
    }
}

pub struct S3Bucket {
    pub name: String,
    pub public_website: Option<PublicWebsite>,
    pub no_custom_cleanup: bool,
    pub access_control: String,
}

impl Default for S3Bucket {
    fn default() -> Self {
        Self {
            name: "".into(),
            public_website: None,
            no_custom_cleanup: false,
            access_control: "Private".into(),
        }
    }
}

impl From<AttributeValue> for S3Bucket {
    fn from(value: AttributeValue) -> Self {
        let map = match value {
            AttributeValue::Map(m) => { m },
            _ => {
                panic!("S3 Bucket attribute values must be a map. Instead found {:?}", value);
            }
        };
        let mut out = S3Bucket::default();
        for (key, val) in map {
            match key.as_str() {
                "name" => {
                    out.name = val.assert_str("name");
                },
                "access_control" => {
                    out.access_control = val.assert_str("access_control");
                }
                "public_website" => {
                    out.access_control = "PublicRead".into();
                    let website_conf = val.assert_map("public_website");
                    let mut public_website = PublicWebsite::default();
                    for (key, val) in website_conf {
                        match key.as_str() {
                            "index_document" => {
                                public_website.index_document = val.assert_str("index_document");
                            },
                            "error_document" => {
                                public_website.error_document = val.assert_str("error_document");
                            },
                            _ => panic!("Unexpected key '{}' in S3 bucket website configuration", key),
                        }
                    }
                    out.public_website = Some(public_website);
                },
                "no_custom_cleanup" => {
                    let no_custom_cleanup_val = val.assert_str("no_custom_cleanup");
                    match no_custom_cleanup_val.as_str() {
                        "true" => out.no_custom_cleanup = true,
                        "false" => out.no_custom_cleanup = false,
                        _ => panic!("Unexpected value '{no_custom_cleanup_val}' for no_custom_cleanup"),
                    }
                }
                _ => panic!("Unexpected key '{key}' in S3 bucket attributes"),
            }
        }
        if out.name.is_empty() {
            panic!("Must provide a bucket name");
        }
        out
    }
}

pub fn add_s3_bucket_resource(s3_conf: S3Bucket) {
    let bucket_name = &s3_conf.name;
    let resource_name = bucket_name.replace("_", "").replace(".", "").replace("-", "");
    let access_control = &s3_conf.access_control;
    let mut out = format!("
    S3Bucket{resource_name}:
        Type: AWS::S3::Bucket
        Properties:
            AccessControl: {access_control}
            BucketName: {bucket_name}\n");
    if let Some(conf) = &s3_conf.public_website {
        let index = &conf.index_document;
        let error = &conf.error_document;
        out.push_str(&format!("            WebsiteConfiguration:
                IndexDocument: {index}
                ErrorDocument: {error}\n"
        ));
        out.push_str(&format!("
    S3BucketWebsitePolicy{resource_name}:
        Type: AWS::S3::BucketPolicy
        Properties:
            Bucket: !Ref S3Bucket{resource_name}
            PolicyDocument:
                Version: '2012-10-17'
                Statement:
                  - Action: 's3:GetObject'
                    Effect: Allow
                    Principal: '*'
                    Resource: !Sub 'arn:aws:s3:::${{S3Bucket{resource_name}}}/*'\n"
        ));
    }
    // s3 buckets in cloudformation cannot be deleted if they contain objects.
    // as long as the user doesnt disable "no_custom_cleanup", we will
    // add a custom resource to cleanup the bucket before it can be deleted:
    if !s3_conf.no_custom_cleanup {
        out.push_str(&format!("
    S3DeleteLambdaRole{resource_name}:
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
            Policies:
            - PolicyName: lambda_generated_policy
              PolicyDocument:
                  Version: '2012-10-17'
                  Statement:
                    - Effect: Allow
                      Action:
                        - s3:DeleteObject
                        - s3:ListBucket
                      Resource:
                        - !Sub 'arn:aws:s3:::${{S3Bucket{resource_name}}}/*'
                        - !Sub 'arn:aws:s3:::${{S3Bucket{resource_name}}}'
    S3DeleteLambda{resource_name}:
        Type: AWS::Lambda::Function
        Properties:
            Runtime: nodejs12.x
            Role: !GetAtt S3DeleteLambdaRole{resource_name}.Arn
            Handler: index.handler
            Code:
                ZipFile: |
                    var AWS = require('aws-sdk')
                    var response = require('cfn-response')
                    const s3 = new AWS.S3({{}});
                    async function listObjects(bucketName) {{
                        const data = await s3.listObjects({{ Bucket: bucketName }}).promise();
                        const objects = data.Contents;
                        for (let obj of objects) {{
                            console.log(obj.Key);
                            await s3.deleteObject({{ Bucket: bucketName, Key: obj.Key }}).promise();
                        }}
                        console.log(`Successfully deleted ${{objects.length}} objects from S3 bucket`);
                    }}
                    exports.handler = async function(event, context) {{
                        console.log('REQUEST RECEIVED:' + JSON.stringify(event))
                        let responseType = response.SUCCESS
                        if (event.RequestType == 'Delete') {{
                            try {{
                                await listObjects(event.ResourceProperties.BucketName);
                            }} catch (err) {{
                                console.log(`Error deleting objects from S3 bucket: ${{err}}`);
                                responseType = response.FAILED
                            }}
                        }}
                        await response.send(event, context, responseType)
                    }}
    S3CleanupResource:
        Type: Custom::cleanupbucket{resource_name}
        Properties:
            ServiceToken: !GetAtt S3DeleteLambda{resource_name}.Arn
            BucketName: !Ref S3Bucket{resource_name}\n"
        ));
    }

    unsafe {
        RESOURCES.push(out);
    }
}
