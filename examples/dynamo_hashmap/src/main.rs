use serde::{Serialize, Deserialize};

const _: &'static str = include_str!("../aws_dynamodb.rhai");

hira::set_stack_name!("example-dynamo-hashmap");
hira::const_from_dot_env!(BUILD_BUCKET);
hira::set_build_bucket!(BUILD_BUCKET);

#[derive(Serialize, Deserialize, Default)]
pub struct MyGameState {
    pub num_players: usize,
    pub active: bool,
}

#[hira::module("hira:aws_dynamodb", {})]
pub mod mygametable {
    use super::MyGameState;
    type TableDef = HashMap<String, Mutex<MyGameState>>;
}

#[hira::module("hira:aws_lambda", {
    policy_statements: [
        { action: "dynamodb:*Item", resource: mygametable::TABLE_ARN }
    ]
})]
pub async fn mylambda(_: serde_json::Value) -> Result<String> {
    let resp = mygametable::get_or_create_item("gameid123", |is_new, myobj| {
        if !is_new {
            myobj.active = true;
        }
        myobj.num_players += 1;
    }).await;
    println!("{:?}", resp);
    Ok("".into())
}

hira::close!();
