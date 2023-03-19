fn mod_macro(x) {
    let mod_name = x.get_name();
    let user_data_type = "";
    try {
        user_data_type = x.get_encapsulated_value("type TableDef = HashMap<String, Mutex<*>>;");
    } catch (err) {
        throw `Failed to find TableDef for ${mod_name}. Must provide a type def inside the module such as "type TableDef = HashMap<String, Mutext<YourType>>;`;
    }
    x.add_code_inside(`
    use std::collections::HashMap;
    use std::sync::Mutex;
    `);

    let lock_seconds = "10";
    if HIRA_MOD_INPUT.contains("lock_seconds") {
        lock_seconds = HIRA_MOD_INPUT.lock_seconds;
    }
    let table_name = mod_name;
    if HIRA_MOD_INPUT.contains("table_name") {
        table_name = HIRA_MOD_INPUT.table_name;
    }

    x.add_code_inside(`pub const LOCK_TS: u64 = ${lock_seconds};`);
    x.add_code_inside(`pub const TABLE_NAME: &'static str = "${table_name}";`);

    // type check! this helps the user verify what traits they are missing for their object:
    x.add_code_inside(`
    #[cfg(debug_assertions)]
    trait DynamoData: serde::de::DeserializeOwned + serde::Serialize + Default {}
    #[cfg(debug_assertions)]
    impl DynamoData for ${user_data_type} {}
    `);
    // similarly, so the user doesnt get warnings:
    x.add_code_inside(`
    pub fn _convince_compiler(_: TableDef) {}
    `);

    // the part that the user can interface with in their code
    x.add_code_inside(`
    pub async fn get_or_create_item<S: AsRef<str>, F: FnMut(bool, &mut ${user_data_type})>(key: S, mut cb: F) -> Result<(), Box<dyn std::error::Error>> {
        let client = super::make_dynamo_client().await;
        super::_hira_dynamo_use_item(&client, self::TABLE_NAME, key.as_ref(), self::LOCK_TS, |is_new, item: &mut ${user_data_type}| {
            cb(is_new, item);
            Ok(())
        }).await
    }
    pub async fn get_or_create_item_fallible<S: AsRef<str>, F: FnMut(bool, &mut ${user_data_type}) -> Result<(), Box<dyn std::error::Error>>>(key: S, mut cb: F) -> Result<(), Box<dyn std::error::Error>> {
        let client = super::make_dynamo_client().await;
        super::_hira_dynamo_use_item(&client, self::TABLE_NAME, key.as_ref(), self::LOCK_TS, |is_new, item: &mut ${user_data_type}| {
            cb(is_new, item)
        }).await
    }
    /// This simply performs a dynamoDB getItem. If you'd like to modify the item afterwards, it is
    /// highly recommended to use get_or_create_item which handles locking for you.
    pub async fn get_item<S: AsRef<str>>(key: S) -> Result<${user_data_type}, Box<dyn std::error::Error>> {
        let client = super::make_dynamo_client().await;
        let item_out = client.get_item().table_name(self::TABLE_NAME).key("hira_pkey", aws_sdk_dynamodb::model::AttributeValue::S(key.as_ref().into()))
            .send().await?;
        let attributes = match item_out.item {
            Some(a) => a,
            None => return Err(super::make_err("DDB did not return attributes even though we requested them")),
        };
        let mut map: serde_json::Map<String, serde_json::Value> = Default::default();
        for (key, val) in attributes {
            if key == "hira_pkey" || key == "hira_lock_ts" || key == "hira_ttl_ts" {
                continue;
            }
            map.insert(key.clone(), super::attribute_val_to_serde_val(val)?);
        }
        if map.is_empty() {
            return Err(super::make_err("DDB item exists but is empty"));
        }
        let val = serde_json::Value::Object(map);
        let obj: ${user_data_type} = serde_json::from_value(val)?;
        Ok(obj)
    }
    pub async fn delete_item<S: AsRef<str>>(key: S) -> Result<(), Box<dyn std::error::Error>> {
        let client = super::make_dynamo_client().await;
        let item_out = client.delete_item().table_name(self::TABLE_NAME).key("hira_pkey", aws_sdk_dynamodb::model::AttributeValue::S(key.as_ref().into()))
            .send().await?;
        Ok(())
    }
    `);

    // if user adds multiple tables, this only appears once in their program:
    x.add_code_after(`
pub async fn _hira_dynamo_use_item<T: Default + serde::Serialize + serde::de::DeserializeOwned, F: FnMut(bool, &mut T) -> Result<(), Box<dyn std::error::Error>>>(
    client: &aws_sdk_dynamodb::Client,
    table_name: &str,
    key: &str,
    lock_seconds: u64,
    mut cb: F
) -> Result<(), Box<dyn std::error::Error>> {
    let current_timestamp = current_timestamp();
    let last_lock_time = format!("{}", current_timestamp - lock_seconds);
    let current_lock_time = format!("{}", current_timestamp);
    // this will create a new item if one doesnt exist.
    // otherwise, it will simply set the lock, and return the object
    let update_req = client.update_item()
        .table_name(table_name)
        .key("hira_pkey", aws_sdk_dynamodb::model::AttributeValue::S(key.into()))
        .update_expression("SET hira_lock_ts = :current_lock")
        .condition_expression("attribute_not_exists(pkey) OR hira_lock_ts < :last_lock")
        .return_values(aws_sdk_dynamodb::model::ReturnValue::AllNew)
        .expression_attribute_values(":last_lock", aws_sdk_dynamodb::model::AttributeValue::N(last_lock_time))
        .expression_attribute_values(":current_lock", aws_sdk_dynamodb::model::AttributeValue::N(current_lock_time.clone()))
        .send().await;
    let item = update_req?;
    let attributes = match item.attributes {
        Some(a) => a,
        None => return Err(make_err("DDB did not return attributes even though we requested them")),
    };

    let mut map: serde_json::Map<String, serde_json::Value> = Default::default();
    for (key, val) in attributes {
        // dont include the hira specific keys. the rest of the attributes
        // are the user's real attributes, and should be deserializable to their object.
        if key == "hira_pkey" || key == "hira_lock_ts" || key == "hira_ttl_ts" {
            continue;
        }
        map.insert(key.clone(), attribute_val_to_serde_val(val)?);
    }
    // if the map is empty, this means that the item did not exist,
    // and we just inserted an item that only contains the primary key, and a lock.
    // instead of trying to deserialize it (because it will most likely fail),
    // we will use a default object
    let (is_new, mut user_obj) = if map.is_empty() {
        (true, T::default())
    } else {
        let val = serde_json::Value::Object(map);
        let obj: T = serde_json::from_value(val)?;
        (false, obj)
    };
    // if the user's callback fails, thats ok:
    // the object will just be locked for up to LOCK_TS seconds.
    cb(is_new, &mut user_obj)?;

    // now we serialize back to an item we can put into dynamo.
    let mut val = serde_json::to_value(user_obj)?;
    let map = match val.as_object_mut() {
        Some(m) => m,
        None => return Err(make_err("hira error. DDB Item is not deserializable as an object")),
    };
    let mut out_map = std::collections::HashMap::new();
    for (key, val) in map {
        out_map.insert(key.clone(), serde_val_to_attribute_val(val.take()));
    }
    client.put_item()
        .table_name(table_name)
        .condition_expression("hira_lock_ts = :current_lock")
        .expression_attribute_values(":current_lock", aws_sdk_dynamodb::model::AttributeValue::N(current_lock_time))
        .set_item(Some(out_map))
        .item("hira_pkey", aws_sdk_dynamodb::model::AttributeValue::S(key.into()))
        // we set the lock to 0 because we no longer need this object.
        .item("hira_lock_ts", aws_sdk_dynamodb::model::AttributeValue::N("0".into()))
        .send().await?;

    Ok(())
}
pub async fn make_dynamo_client() -> aws_sdk_dynamodb::Client {
    let region = "${HIRA_DEPLOY_REGION}";
    let region_provider = aws_config::meta::region::RegionProviderChain::first_try(region);
    let sdk_config = aws_config::from_env().region(region_provider).load().await;
    aws_sdk_dynamodb::Client::new(&sdk_config)
}

pub fn make_err(s: &str) -> Box<dyn std::error::Error> {
    let err = std::io::ErrorKind::Other;
    let err = std::io::Error::new(err, s);
    Box::new(err)
}

pub fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!("uhhh time went backwards...")
    }
}

pub fn serde_val_to_attribute_val(serde_val: serde_json::Value) -> aws_sdk_dynamodb::model::AttributeValue {
    match serde_val {
        serde_json::Value::Null => {
            aws_sdk_dynamodb::model::AttributeValue::Null(true)
        }
        serde_json::Value::Bool(b) => {
            aws_sdk_dynamodb::model::AttributeValue::Bool(b)
        }
        serde_json::Value::Number(n) => {
            aws_sdk_dynamodb::model::AttributeValue::N(n.to_string())
        }
        serde_json::Value::String(s) => {
            aws_sdk_dynamodb::model::AttributeValue::S(s)
        }
        serde_json::Value::Array(arr) => {
            let mut out = vec![];
            for item in arr {
                out.push(serde_val_to_attribute_val(item));
            }
            aws_sdk_dynamodb::model::AttributeValue::L(out)
        }
        serde_json::Value::Object(map) => {
            let mut out_map = std::collections::HashMap::new();
            for (key, val) in map {
                out_map.insert(key, serde_val_to_attribute_val(val));
            }
            aws_sdk_dynamodb::model::AttributeValue::M(out_map)
        }
    }
}

pub fn attribute_val_to_serde_val(attr_val: aws_sdk_dynamodb::model::AttributeValue) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let out = match attr_val {
        aws_sdk_dynamodb::model::AttributeValue::B(b) => {
            let data = b.into_inner();
            serde_json::Value::from(data)
        }
        aws_sdk_dynamodb::model::AttributeValue::Bool(b) => {
            serde_json::Value::from(b)
        }
        aws_sdk_dynamodb::model::AttributeValue::Bs(b) => {
            let mut out = vec![];
            for blob in b {
                out.push(serde_json::Value::from(blob.into_inner()));
            }
            serde_json::Value::from(out)
        }
        aws_sdk_dynamodb::model::AttributeValue::L(list) => {
            let mut out = vec![];
            for item in list {
                out.push(attribute_val_to_serde_val(item)?);
            }
            serde_json::Value::from(out)
        }
        aws_sdk_dynamodb::model::AttributeValue::M(map) => {
            let mut out_map = serde_json::Map::new();
            for (key, inner_val) in map {
                out_map.insert(key, attribute_val_to_serde_val(inner_val)?);
            }
            serde_json::Value::from(out_map)
        }
        aws_sdk_dynamodb::model::AttributeValue::N(num) => {
            if num.contains(".") {
                let n = num.parse::<f64>()?;
                serde_json::Value::from(n)
            } else {
                let n = num.parse::<u64>()?;
                serde_json::Value::from(n)
            }
        }
        aws_sdk_dynamodb::model::AttributeValue::Ns(num_set) => {
            let mut out = vec![];
            for num in num_set {
                if num.contains(".") {
                    let n = num.parse::<f64>()?;
                    out.push(serde_json::Value::from(n));
                } else {
                    let n = num.parse::<u64>()?;
                    out.push(serde_json::Value::from(n));
                }   
            }
            serde_json::Value::from(out)
        }
        aws_sdk_dynamodb::model::AttributeValue::Null(_) => {
            serde_json::Value::Null
        }
        aws_sdk_dynamodb::model::AttributeValue::S(s) => {
            serde_json::Value::String(s)
        }
        aws_sdk_dynamodb::model::AttributeValue::Ss(string_set) => {
            serde_json::Value::from(string_set)
        }
        x => return Err(make_err(&format!("Unknown attribute {:?}", x))),
    };
    Ok(out)
}
    `);

    x.set_global_const("TABLE_ARN", `arn:aws:dynamodb:${HIRA_DEPLOY_REGION}:*:table/${table_name}`);

    add_to_cfn(`
    Dynamo${table_name}:
        Type: AWS::DynamoDB::Table
        Properties:
            KeySchema:
              -
                AttributeName: hira_pkey
                KeyType: HASH
            AttributeDefinitions:
              -
                AttributeName: hira_pkey
                AttributeType: 'S'
            BillingMode: PAY_PER_REQUEST
            TableName: ${table_name}`);
    x
    // TODO:
    // TimeToLiveSpecification:
    //     AttributeName: hira_ttl_ts
    //     Enabled: true
}
