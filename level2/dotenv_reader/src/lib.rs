use hira_lib::level0::L0Core;

#[hira::hira]
pub mod dotenv_reader {
    use super::L0Core;

    #[derive(Default)]
    #[cfg_attr(feature = "web", derive(serde::Serialize, serde::Deserialize))]
    pub struct Input {
        /// the path to your .env file.
        /// it should be relative to the root of your crate, ie:
        /// the same directory as your Cargo.toml file.
        pub dotenv_path: String,
    }

    pub fn config(myinp: &mut Input, l0core: &mut L0Core) {
        l0core.set_dotenv_location(&myinp.dotenv_path);
    }
}
