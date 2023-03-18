fn match_macro(x) {
    let cert_arn = "";
    if !(HIRA_MOD_INPUT.contains("acm_cert_arn")) {
        throw "Must provide acm_cert_arn to module attributes";
    }
    cert_arn = HIRA_MOD_INPUT.acm_cert_arn;
    let module_name = x.get_name();
    let stuff = x.get_match_content();
    let match_against = stuff.match_against;
    let match_body = stuff.match_body;
    if match_against.len() != 2 || match_against[1] != "path" {
        throw "Match statement must contain 2 parts: the domain, and the keyword 'path'";
    }
    let match_domain = match_against[0];
    // if the user provides the hosted zone as theirwebsite.com
    // this splits into 2, and therefore the max number of '.' delimiters
    // in any subdomain is 2. ie, they can have 1 extra dot for a subdomain, so they
    // can do anything like *.theirwebsite.com, or explicitly subdomain1.theirwebsite.com, etc.
    let valid_num_dots = match_domain.split(".").len();

    let record_cdn_group = #{};
    for item in match_body {
        let m = item["match"];
        let origin = item["result"];
        if m.len() != 2 {
            throw "Match group must contain 2 strings: the domain, and the path";
        }
        if !(m[0].ends_with(match_domain)) {
            throw m[0] + " is not a subset of the specified domain:" + match_domain;
        }
        let num_dots = m[0].split(".").len() - 1;
        if num_dots > valid_num_dots {
            throw `${m[0]} contains ${num_dots} sections, whereas ${match_domain} only permits ${valid_num_dots}. There is a limitation with route53 that you may only have 1 wildcard subdomain."`;
        }
        if record_cdn_group.contains(m[0]) {
            let val = record_cdn_group[m[0]];
            val.push(#{ route: m[1], origin: origin });
            record_cdn_group[m[0]] = val;
        } else {
            record_cdn_group[m[0]] = [#{ route: m[1], origin: origin }];
        }
    }


    let index = 0;
    for host in record_cdn_group.keys() {
        let cdn_routes = record_cdn_group[host];
        let default_route = #{};
        let secondary_routes = [];
        for route in cdn_routes {
            if route.origin.len() < 2 {
                throw `${host} route '${route.route}' has an invalid origin config. Must contain at least 2 strings. The first is the origin domain, and the 2nd is the default origin path.`
            }
            if route.origin[1].ends_with("/") {
                throw `${host} route '${route.route}' has an origin path '${route.origin[1]}' that ends in a trailing slash. Origin paths cannot end in a trailing slash`;
            }
            if route.route == "*" {
                default_route = route;
            } else {
                secondary_routes.push(route);
            }
        }

        if default_route.is_empty() {
            throw `${host} does not contain a default route. Each different host must contain at least one match item where the path component is '*'`;
        }

        let default_origin_path = "";
        if default_route.origin[1] != "*" {
            default_origin_path = `                  OriginPath: ${default_route.origin[1]}`;
        }
        let default_origin_policy = "https-only";
        if default_route.origin.len() > 2 {
            let extra_params = default_route.origin[2];
            for key in extra_params.split(",") {
                if key.contains("originProtocol") {
                    default_origin_policy = "http-only";
                }
            }
        }

        let origin_index = 0;
        let cache_behaviors = "";
        let origins = "";
        for route in secondary_routes {
            origin_index += 1;
            let origin_id = `origin${origin_index}`;
            if cache_behaviors.is_empty() {
                cache_behaviors = "                CacheBehaviors:\n";
            }
            cache_behaviors += `
                - TargetOriginId: ${origin_id}
                  ViewerProtocolPolicy: redirect-to-https
                  CachePolicyId: 658327ea-f89d-4fab-a63d-7e88639e58f6
                  PathPattern: ${route.route}
            `;
            origins += `
                - CustomOriginConfig:
                    HTTPPort: 80
                    HTTPSPort: 443
                    OriginProtocolPolicy: http-only
                  DomainName: ${route.origin[0]}
                  Id: ${origin_id}`;
            if route.origin[1] != "*" {
                origins += "\n" + "                  OriginPath: " + route.origin[1];
            }
        }

        add_to_cfn(`
    CDN${module_name}${index}:
        Type: AWS::CloudFront::Distribution
        Properties:
            DistributionConfig:
                Enabled: true
                ViewerCertificate:
                    AcmCertificateArn: ${cert_arn}
                    MinimumProtocolVersion: TLSv1.2_2021
                    SslSupportMethod: sni-only
                Aliases:
                - ${host}
                DefaultCacheBehavior:
                    TargetOriginId: default-origin
                    ViewerProtocolPolicy: redirect-to-https
                    CachePolicyId: 658327ea-f89d-4fab-a63d-7e88639e58f6
                Origins:
                - CustomOriginConfig:
                    HTTPPort: 80
                    HTTPSPort: 443
                    OriginProtocolPolicy: ${default_origin_policy}
                  DomainName: ${default_route.origin[0]}
                  Id: default-origin
${default_origin_path}
${origins}
${cache_behaviors}
        `);
        add_to_cfn(`
    R53${module_name}${index}:
        Type: AWS::Route53::RecordSet
        Properties:
            AliasTarget:
                DNSName: !GetAtt CDN${module_name}${index}.DomainName
                HostedZoneId: Z2FDTNDATAQYW2
            HostedZoneName: ${match_domain}.
            Comment: ${host}
            Name: ${host}
            Type: A
        `);
        index += 1;
    }

    x
}
