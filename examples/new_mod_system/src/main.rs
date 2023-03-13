// this is not necessary, but its useful for debugging custom modules
// as it forces each build to actually rebuild. Otherwise, the successful build
// gets cached even if the rhai script changes.
const _: &'static str = include_str!("../custom_s3mod.rhai");

#[hira::module("./custom_s3mod.rhai", {
    bucket_name: "mys3buckettestthingyahrhai",
    public_website: {},
})]
pub mod thing {
    pub async fn _init() {
        let website = include_str!("../index.html");
        let client = make_s3_client().await;
        self::put_object_builder(&client, "index.html", website.into())
            .content_type("text/html")
            .send().await
            .expect("failed to write index.html");
    }
}

hira::close!();
