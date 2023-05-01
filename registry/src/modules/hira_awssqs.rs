#[hira::hira] use {
    hira_awscfn,
};

#[allow(dead_code)]
const HIRA_MODULE_NAME: &'static str = "hira_awssqs";

#[derive(Default)]
pub struct SqsInput {
    /// The logical name of this resource in cloudformation.
    /// By default this is set to Q{queue_name}. (and we sanitize
    /// to be alphanumeric, and up to 256 characters).
    pub resource_name: String,
    /// Default 0s => no delay.
    /// Set this value in order to add a default delay time for each
    /// message that was sent to this queue prior to it becoming available
    /// for a receiver. Valid values 0s - 900s (15 minutes).
    pub default_queue_delay_s: u32,
    /// By default SQS allows messages of up to 256kb. You can enforce a smaller
    /// size optionally. Valid values are between 1024 - 262144 (256kb).
    /// Default is 262144
    pub max_message_size_bytes: u32, 

    /// SQS will only store messages up to 4 days by default. Optionally you can
    /// lower or raise this retention period. Valid values are
    /// 60 (1 minute) - 1209600 (14 days)
    pub max_retention_period_s: u32,

    /// Give a name to the queue. By default hira sets this to
    /// the name of your module. Set this to an
    /// empty string to rely on CloudFormation creating a random name for you.
    pub queue_name: String,

    /// When you call the ReceiveMessage API action, SQS will wait this many seconds
    /// before returning a response. Longer values mean you can get more messages
    /// back per call. Valid values are between 0 - 20.
    /// Default 0.
    pub receive_message_wait_time_s: u32,

    /// Controls whether the queue will be encrypted with AWS SSE.
    /// Default is false.
    pub managed_sse_enabled: bool,

    /// When a message is received from SQS, it is still in the queue, and other consumers
    /// could receive the same message. By specifying a visibility timeout you can tell SQS
    /// to not deliver this message to other consumers for up to a given time in seconds.
    /// Default is 30 seconds. Valid values are 0 - 43200 (12 hours)
    /// See: https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-visibility-timeout.html
    pub visibility_timeout_s: u32,

    /// the region this queue will be deployed in.
    /// Defaults to us-west-2
    pub region: String,
}

impl SqsInput {
    pub fn new(name: &str) -> Self {
        let mut out = Self::default();
        out.default_queue_delay_s = 0;
        out.max_message_size_bytes = 262144;
        out.max_retention_period_s = 345600;
        out.resource_name = format!("Q{name}");
        out.resource_name = out.resource_name.replace("_", "");
        out.resource_name.truncate(256);
        out.queue_name = name.to_string();
        out.queue_name.truncate(80);
        out.receive_message_wait_time_s = 0;
        out.managed_sse_enabled = false;
        out.visibility_timeout_s = 30;
        out.region = "us-west-2".to_string();
        out
    }

    pub fn is_valid(&self) -> Option<String> {
        if !self.queue_name.is_empty() {
            if self.queue_name.len() > 80 {
                return Some(format!("Invalid queue name {:?}\nmust be <= 80 characters", self.queue_name));
            }
            if self.queue_name.ends_with(".fifo") {
                return Some(format!("Invalid queue name {:?}\nFIFO queues are not supported yet", self.queue_name));
            }
            let valid_chars = self.queue_name.chars().all(|x| {
                x.is_ascii_alphanumeric() || x == '-' || x == '_'
            });
            if !valid_chars {
                return Some(format!("Invalid queue name {:?}\nOnly alphanumeric characters and '_' and '-' are supported", self.queue_name));
            }
        }
        if self.default_queue_delay_s > 900 {
            return Some(format!("Invalid default queue delay {:?}\nValid range 0 - 900", self.default_queue_delay_s));
        }
        if self.max_message_size_bytes < 1024 || self.max_message_size_bytes > 262144 {
            return Some(format!("Invalid max message size {:?}\nValid range 1024 - 262144", self.max_message_size_bytes));
        }
        if self.max_retention_period_s < 60 || self.max_retention_period_s > 1209600 {
            return Some(format!("Invalid max retention period {:?}\nValid range 60 - 1209600", self.max_retention_period_s));
        }
        if self.receive_message_wait_time_s > 20 {
            return Some(format!("Invalid receive message wait time {:?}\nValid range 0 - 20", self.receive_message_wait_time_s));
        }
        if self.visibility_timeout_s > 43200 {
            return Some(format!("Invalid visibility timeout {:?}\nValid range 0 - 43200", self.visibility_timeout_s));
        }
        if let Some(x) = hira_awscfn::verify_resource_name(&self.resource_name) {
            return Some(x);
        }
        None
    }

    pub fn output_cfn(&self) -> String {
        let Self {
            resource_name,
            default_queue_delay_s,
            max_message_size_bytes,
            max_retention_period_s,
            queue_name,
            receive_message_wait_time_s,
            managed_sse_enabled,
            visibility_timeout_s,
            ..
        } = self;

        let queue_name = if queue_name.is_empty() {
            "# queue name omitted. Cfn will randomly generate it".to_string()
        } else {
            format!("QueueName: {queue_name}")
        };

        let x = format!(
r#"    {resource_name}:
        Type: AWS::SQS::Queue
        Properties:
            DelaySeconds: {default_queue_delay_s}
            MaximumMessageSize: {max_message_size_bytes}
            MessageRetentionPeriod: {max_retention_period_s}
            {queue_name}
            ReceiveMessageWaitTimeSeconds: {receive_message_wait_time_s}
            SqsManagedSseEnabled: {managed_sse_enabled}
            VisibilityTimeout: {visibility_timeout_s}
"#);

        x
    }
}

#[allow(dead_code)]
type ExportType = SqsInput;

pub fn wasm_entrypoint(obj: &mut LibraryObj, cb: fn(&mut SqsInput)) {
    let name = match &obj.user_data {
        UserData::Module { name, ..} => {
            name
        }
        _ => {
            obj.compile_error("This module can only be used on mod defs. Eg expected usage:\n```\n#[hira(|obj: &mut hira_awssqs::SqsInput| {})]\nmod mysqs_queue { ... }\n```");
            return;
        }
    };
    let mut queue_input = SqsInput::new(name);
    cb(&mut queue_input);
    if let Some(err_msg) = queue_input.is_valid() {
        obj.compile_error(&err_msg);
        return;
    }

    let region = &queue_input.region;
    let resources = queue_input.output_cfn();
    hira_awscfn::output_cfn_file(obj, region, &[], resources);
}
