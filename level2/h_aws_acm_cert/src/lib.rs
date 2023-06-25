use hira_lib::level0::*;
use aws_cfn_stack::aws_cfn_stack;

/// This module defines and creates an AWS ACM certificate. This module only works if the following is true:
/// - The domain you'd like to get a certificate for is hosted in Amazon Route 53
/// - The domain resides in your AWS account.
/// - You are using DNS validation.
#[hira::hira]
pub mod aws_acm_cert {
    extern crate certificate_manager;
    extern crate cfn_resources;

    use super::L0Core;
    use super::aws_cfn_stack;
    use self::certificate_manager::certificate::CertificateValidationMethodEnum;
    // use self::aws_cfn_stack::ResourceOutput;
    // use self::cfn_resources::StrVal;
    // use self::cfn_resources::ToOptStrVal;
    // use self::cfn_resources::get_ref;
    // use self::certificate_manager::certificate::DomainValidationOption;

    pub mod outputs {
        /// this is the logical name in cloudformation for your cert.
        /// Reference this name in other resources that rely on it,
        /// for example, perhaps a cloudfront distribution that wants to reference the cert ARN.
        pub const LOGICAL_CERT_NAME: &str = "UNDEFINED";
    }

    #[derive(Default)]
    pub struct Input {
        /// the domain you're requesting a certificate for. Must be fully qualified. Can have 1 optional wildcard.
        /// Examples of valid values:
        /// - www.mysite.com
        /// - multiple.sub.domains.mysite.com
        /// - mysite.com
        /// - *.mysite.com
        /// Examples of invalid values:
        /// - *.something.*.mysite.com
        /// - cannotendwithdot.com.
        pub domain_name: String,

        // /// The hosted zone ID of where your domain is hosted in Route53.
        // /// Must be provided as the actual ID without the `/hostedzone/` prefix.
        // pub hosted_zone_id: String,
    }

    pub fn config(self_input: &mut Input, l0core: &mut L0Core, stackinp: &mut aws_cfn_stack::Input) {
        if self_input.domain_name.is_empty() {
            l0core.compiler_error(&format!("Must provide a domain name"));
            return;
        }
        // if self_input.hosted_zone_id.is_empty() {
        //     core.compiler_error(&format!("Must provide the hosted zone ID of where your domain resides"));
        //     return;
        // }
        // TODO: optional domain validation?
        if self_input.domain_name.contains("*") {
            let matches = self_input.domain_name.matches("*");
            if matches.count() > 1 {
                l0core.compiler_error(&format!("Must only provide 1 wildcard. {} is invalid.", self_input.domain_name));
                return;
            }
            if !self_input.domain_name.starts_with("*") {
                l0core.compiler_error(&format!("If using a wildcard, it must be the first component of your domain, eg: \"*.something.com\". {} is invalid.", self_input.domain_name));
                return;
            }
        }
        let cert = certificate_manager::certificate::CfnCertificate {
            domain_name: self_input.domain_name.clone().into(),
            validation_method: CertificateValidationMethodEnum::Dns.into(),
            // domain_validation_options: vec![
            //     DomainValidationOption {
            //         domain_name: self_input.domain_name.clone().into(),
            //         hosted_zone_id: self_input.hosted_zone_id.clone().to_str_val(),
            //         ..Default::default()
            //     }
            // ].into(),
            ..Default::default()
        };
        let user_mod_name = l0core.users_module_name();
        let logical_cert_name = format!("hiragencert{user_mod_name}");
        let logical_cert_name = logical_cert_name.replace("_", "");

        let resource = aws_cfn_stack::Resource {
            name: logical_cert_name.clone(),
            properties: Box::new(cert) as _,
        };
        l0core.set_output("LOGICAL_CERT_NAME", &logical_cert_name);
        stackinp.resources.push(resource);
    }
}
