// this is not necessary, but its useful for debugging custom modules
// as it forces each build to actually rebuild. Otherwise, the successful build
// gets cached even if the rhai script changes.
const _: &'static str = include_str!("../custom_s3mod.rhai");

#[hira::module("./custom_s3mod.rhai", {
    bucket_name: "mys3buckettestthingyahrhai"
})]
pub mod thing {
    pub async fn _init() {
        self::put_object("helloworld.txt", "helloworld".into()).await.expect("failed to write");
    }
}

hira::close!();
