use hira_lib::level0::*;


#[hira::hira]
pub mod echo {
    use super::{L0RuntimeCreator, L0Core, RuntimeMeta};

    pub const CAPABILITY_PARAMS: &[(&str, &[&str])] = &[
        ("RUNTIME", &[""]),
    ];

    #[derive(Default)]
    pub struct Input {
        pub echo: String,
    }

    pub fn config(self_input: &mut Input, l0core: &mut L0Core, runtimer: &mut L0RuntimeCreator) {
        let meta = RuntimeMeta {
            cargo_cmd: Default::default(), target: Default::default(), profile: Default::default(), no_tokio_async_runtime: true
        };
        runtimer.add_to_runtime_ex(&l0core.users_module_name(), format!("println!(r#\"{}\"#)", self_input.echo), meta);
    }
}
