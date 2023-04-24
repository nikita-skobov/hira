#[cfg(test)]
mod tests {
    use crate::modules::cloudfront::*;

    fn mptrn(s: &str) -> Option<String> {
        Some(s.to_string())
    }
    fn wild() -> Option<String> {
        None
    }

    #[test]
    fn cf_requires_2_tuple_input() {
        let cb = |_a: &mut CloudfrontInput| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Match {
            expr: vec!["abc".to_string()],
            name: "".to_string(),
            is_pub: true,
            arms: vec![]
        };
        let _cfninput = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "this module expects the match statement expression to be a tuple of 2 strings");
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Match {
            expr: vec![],
            name: "".to_string(),
            is_pub: true,
            arms: vec![]
        };
        let _cfninput = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "this module expects the match statement expression to be a tuple of 2 strings");
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Match {
            expr: vec!["".to_string(), "path".to_string()],
            name: "".to_string(),
            is_pub: true,
            arms: vec![]
        };
        let _cfninput = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "");
    }

    #[test]
    fn cf_groups_arms_by_domain() {
        let cb = |_a: &mut CloudfrontInput| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Match {
            expr: vec!["example.com".to_string(), "path".to_string()],
            name: "".to_string(),
            is_pub: true,
            arms: vec![
                MatchArm { pattern: vec![mptrn("example.com"), wild()], expr: "dsa".to_string() },
                MatchArm { pattern: vec![mptrn("subdomain.example.com"), mptrn("/hi")], expr: "dsa".to_string() },
                MatchArm { pattern: vec![mptrn("subdomain.example.com"), mptrn("/world")], expr: "dsa".to_string() },
                MatchArm { pattern: vec![mptrn("subdomain.example.com"), wild()], expr: "dsa".to_string() },
            ]
        };
        let cfninput = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(cfninput.num_distributions, 2);
    }

    #[test]
    fn cf_each_domain_must_have_wildcard() {
        let cb = |_a: &mut CloudfrontInput| {};
        let mut obj = LibraryObj::new();
        obj.user_data = UserData::Match {
            expr: vec!["example.com".to_string(), "path".to_string()],
            name: "".to_string(),
            is_pub: true,
            arms: vec![
                MatchArm { pattern: vec![mptrn("example.com"), mptrn("/hey")], expr: "dsa".to_string() },
            ]
        };
        let _cfninput = wasm_entrypoint(&mut obj, cb as _);
        assert_eq!(obj.compiler_error_message, "Distribution for example.com is missing a default path '*'. Ensure one of your match arms has a wildcard '_' for the path component");
    }
}
