use crate::utils::yaml::{parse_yaml_to_json, stringify_json_to_yaml};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use url::Url;

#[derive(Debug, Clone)]
pub struct ConvertedSubscriptionResult {
    pub yaml: String,
    pub profile_title: Option<String>,
    pub subscription_userinfo: Option<String>,
    pub support_url: Option<String>,
    pub web_page_url: Option<String>,
    pub first_node_name: Option<String>,
    pub converted_from: &'static str,
}

const SHARE_PREFIXES: [&str; 7] = [
    "vless://",
    "vmess://",
    "trojan://",
    "ss://",
    "hysteria2://",
    "hy2://",
    "tuic://",
];

pub fn is_share_link_input(input: &str) -> bool {
    let first_line = input
        .trim()
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'));
    let Some(line) = first_line else {
        return false;
    };
    let lower = line.to_lowercase();
    SHARE_PREFIXES.iter().any(|p| lower.starts_with(p))
}

pub fn is_valid_mihomo_object(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    [
        "proxies",
        "proxy-providers",
        "proxy-groups",
        "rules",
        "rule-providers",
        "dns",
        "tun",
        "mixed-port",
    ]
    .iter()
    .any(|k| obj.contains_key(*k))
}

pub fn try_decode_base64_utf8(input: &str) -> Option<String> {
    let cleaned = input.trim_start_matches('\u{FEFF}').trim();
    if cleaned.is_empty() {
        return None;
    }
    // Strip wrapping line breaks only if there are no spaces or scheme prefixes inside
    let compact: String = cleaned.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty() || compact.contains("://") {
        return None;
    }
    let normalized = compact.replace('-', "+").replace('_', "/");
    let pad = (4 - (normalized.len() % 4)) % 4;
    let padded = format!("{normalized}{}", "=".repeat(pad));
    if !padded
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return None;
    }
    let bytes = STANDARD.decode(padded.as_bytes()).ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    if decoded.is_empty() || decoded.contains('\u{FFFD}') {
        return None;
    }
    Some(decoded)
}

fn safe_decode_uri_component(s: &str) -> String {
    urlencoding::decode(s)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| s.to_string())
}

fn make_unique_proxy_name(raw_name: &str, used_names: &mut HashSet<String>) -> String {
    let collapsed: String = raw_name
        .split(['\r', '\n', '\t'])
        .collect::<Vec<_>>()
        .join(" ");
    let base = collapsed.trim();
    let base = if base.is_empty() { "Server" } else { base };
    if used_names.insert(base.to_string()) {
        return base.to_string();
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{base} {idx}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        idx += 1;
    }
}

fn parse_bool_param(val: Option<&str>) -> bool {
    matches!(
        val.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn parse_comma_list(val: Option<&str>) -> Option<Vec<String>> {
    let arr: Vec<String> = val?
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if arr.is_empty() {
        None
    } else {
        Some(arr)
    }
}

fn query_map(url: &Url) -> HashMap<String, String> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn insert_transport_fields(
    proxy: &mut Map<String, Value>,
    network: &str,
    host: Option<&str>,
    path: Option<&str>,
    service_name: Option<&str>,
    mode: Option<&str>,
    header_type: Option<&str>,
) {
    let net = network.trim().to_ascii_lowercase();
    let host = host.map(str::trim).filter(|s| !s.is_empty());
    let path = path.map(str::trim).filter(|s| !s.is_empty());
    let svc = service_name.map(str::trim).filter(|s| !s.is_empty());
    let mode = mode.map(str::trim).filter(|s| !s.is_empty());

    match net.as_str() {
        "ws" => {
            proxy.insert("network".into(), json!("ws"));
            let mut ws = Map::new();
            if let Some(p) = path {
                ws.insert("path".into(), json!(p));
            }
            if let Some(h) = host {
                ws.insert("headers".into(), json!({ "Host": h }));
            }
            if !ws.is_empty() {
                proxy.insert("ws-opts".into(), Value::Object(ws));
            }
        }
        "grpc" => {
            proxy.insert("network".into(), json!("grpc"));
            if let Some(s) = svc.or(path) {
                proxy.insert("grpc-opts".into(), json!({ "grpc-service-name": s }));
            }
        }
        "h2" => {
            proxy.insert("network".into(), json!("h2"));
            let mut h2 = Map::new();
            if let Some(p) = path {
                h2.insert("path".into(), json!(p));
            }
            if let Some(h) = host {
                h2.insert("host".into(), json!([h]));
            }
            if !h2.is_empty() {
                proxy.insert("h2-opts".into(), Value::Object(h2));
            }
        }
        "http" => {
            proxy.insert("network".into(), json!("http"));
            let mut http = Map::new();
            if let Some(p) = path {
                http.insert("path".into(), json!([p]));
            }
            if let Some(h) = host {
                http.insert("headers".into(), json!({ "Host": [h] }));
            }
            if !http.is_empty() {
                proxy.insert("http-opts".into(), Value::Object(http));
            }
        }
        "httpupgrade" => {
            proxy.insert("network".into(), json!("ws"));
            let mut ws = Map::new();
            ws.insert("v2ray-http-upgrade".into(), json!(true));
            if let Some(p) = path {
                ws.insert("path".into(), json!(p));
            }
            if let Some(h) = host {
                ws.insert("headers".into(), json!({ "Host": h }));
            }
            proxy.insert("ws-opts".into(), Value::Object(ws));
        }
        "xhttp" | "splithttp" => {
            proxy.insert("network".into(), json!("xhttp"));
            let mut xh = Map::new();
            if let Some(p) = path {
                xh.insert("path".into(), json!(p));
            }
            if let Some(h) = host {
                xh.insert("host".into(), json!(h));
            }
            if let Some(m) = mode {
                xh.insert("mode".into(), json!(m));
            }
            if !xh.is_empty() {
                proxy.insert("xhttp-opts".into(), Value::Object(xh));
            }
        }
        _ if header_type == Some("http") => {
            proxy.insert("network".into(), json!("http"));
            let mut http = Map::new();
            if let Some(p) = path {
                http.insert("path".into(), json!([p]));
            }
            if let Some(h) = host {
                http.insert("headers".into(), json!({ "Host": [h] }));
            }
            if !http.is_empty() {
                proxy.insert("http-opts".into(), Value::Object(http));
            }
        }
        _ => {
            proxy.insert("network".into(), json!("tcp"));
        }
    }
}

fn parse_vless_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let u = Url::parse(raw).ok()?;
    let server = u.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    let port = u.port().unwrap_or(443);
    let uuid = safe_decode_uri_component(u.username());
    if server.is_empty() || uuid.is_empty() {
        return None;
    }
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| server.clone());
    let name = make_unique_proxy_name(&raw_name, used_names);
    let q = query_map(&u);
    let get = |k: &str| q.get(k).map(String::as_str);

    let security = get("security").unwrap_or("none").to_ascii_lowercase();
    let sni = get("sni").or_else(|| get("peer"));
    let fp = get("fp").or_else(|| get("fingerprint"));
    let alpn = parse_comma_list(get("alpn"));
    let flow = get("flow").filter(|s| !s.is_empty());
    let insecure = parse_bool_param(get("allowInsecure").or_else(|| get("insecure")));

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(name));
    proxy.insert("type".into(), json!("vless"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("uuid".into(), json!(uuid));
    proxy.insert("udp".into(), json!(true));
    insert_transport_fields(
        &mut proxy,
        get("type").unwrap_or("tcp"),
        get("host"),
        get("path"),
        get("serviceName"),
        get("mode"),
        get("headerType"),
    );

    if let Some(f) = flow {
        proxy.insert("flow".into(), json!(f));
    }
    if let Some(pe) = get("packetEncoding").filter(|s| !s.is_empty()) {
        proxy.insert("packet-encoding".into(), json!(pe));
    }
    if security == "tls" || security == "reality" {
        proxy.insert("tls".into(), json!(true));
        if let Some(s) = sni.filter(|s| !s.is_empty()) {
            proxy.insert("servername".into(), json!(s));
        }
        if let Some(f) = fp.filter(|s| !s.is_empty()) {
            proxy.insert("client-fingerprint".into(), json!(f));
        } else if security == "reality" {
            proxy.insert("client-fingerprint".into(), json!("chrome"));
        }
        if let Some(a) = alpn {
            proxy.insert("alpn".into(), json!(a));
        }
        if insecure {
            proxy.insert("skip-cert-verify".into(), json!(true));
        }
        if security == "reality" {
            if let Some(pbk) = get("pbk").filter(|s| !s.is_empty()) {
                let mut reality_opts = Map::new();
                reality_opts.insert("public-key".into(), json!(pbk));
                if let Some(sid) = get("sid") {
                    reality_opts.insert("short-id".into(), json!(sid));
                }
                proxy.insert("reality-opts".into(), Value::Object(reality_opts));
            }
        }
    }
    Some(Value::Object(proxy))
}

fn parse_hysteria2_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let normalized = if raw.len() >= 6 && raw[..6].eq_ignore_ascii_case("hy2://") {
        format!("hysteria2://{}", &raw[6..])
    } else {
        raw.to_string()
    };
    let u = Url::parse(&normalized).ok()?;
    let server = u.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    let port = u.port().unwrap_or(443);
    let password = if !u.username().is_empty() {
        safe_decode_uri_component(u.username())
    } else {
        safe_decode_uri_component(u.password().unwrap_or(""))
    };
    if server.is_empty() || password.is_empty() {
        return None;
    }
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| server.clone());
    let name = make_unique_proxy_name(&raw_name, used_names);
    let q = query_map(&u);
    let get = |k: &str| q.get(k).map(String::as_str);

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(name));
    proxy.insert("type".into(), json!("hysteria2"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("password".into(), json!(password));

    if let Some(mp) = get("mport").or_else(|| get("ports")).filter(|s| !s.is_empty()) {
        proxy.insert("ports".into(), json!(mp));
    }
    if let Some(s) = get("sni").or_else(|| get("peer")).filter(|s| !s.is_empty()) {
        proxy.insert("sni".into(), json!(s));
    }
    if parse_bool_param(get("insecure").or_else(|| get("allowInsecure"))) {
        proxy.insert("skip-cert-verify".into(), json!(true));
    }
    if let Some(obfs) = get("obfs").filter(|s| !s.is_empty() && *s != "none") {
        proxy.insert("obfs".into(), json!(obfs));
        if let Some(op) = get("obfs-password").filter(|s| !s.is_empty()) {
            proxy.insert("obfs-password".into(), json!(op));
        }
    }
    if let Some(a) = parse_comma_list(get("alpn")) {
        proxy.insert("alpn".into(), json!(a));
    }
    if let Some(up) = get("upmbps").or_else(|| get("up")).filter(|s| !s.is_empty()) {
        proxy.insert("up".into(), json!(up));
    }
    if let Some(down) = get("downmbps").or_else(|| get("down")).filter(|s| !s.is_empty()) {
        proxy.insert("down".into(), json!(down));
    }
    if let Some(pin) = get("pinSHA256").filter(|s| !s.is_empty()) {
        proxy.insert("fingerprint".into(), json!(pin));
    }
    Some(Value::Object(proxy))
}

fn parse_trojan_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let u = Url::parse(raw).ok()?;
    let server = u.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    let port = u.port().unwrap_or(443);
    let password = safe_decode_uri_component(u.username());
    if server.is_empty() || password.is_empty() {
        return None;
    }
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| server.clone());
    let name = make_unique_proxy_name(&raw_name, used_names);
    let q = query_map(&u);
    let get = |k: &str| q.get(k).map(String::as_str);

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(name));
    proxy.insert("type".into(), json!("trojan"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("password".into(), json!(password));
    proxy.insert("udp".into(), json!(true));
    insert_transport_fields(
        &mut proxy,
        get("type").unwrap_or("tcp"),
        get("host"),
        get("path"),
        get("serviceName"),
        None,
        None,
    );
    if let Some(s) = get("sni").or_else(|| get("peer")).filter(|s| !s.is_empty()) {
        proxy.insert("sni".into(), json!(s));
    }
    if parse_bool_param(get("allowInsecure").or_else(|| get("insecure"))) {
        proxy.insert("skip-cert-verify".into(), json!(true));
    }
    if let Some(a) = parse_comma_list(get("alpn")) {
        proxy.insert("alpn".into(), json!(a));
    }
    if let Some(f) = get("fp").or_else(|| get("fingerprint")).filter(|s| !s.is_empty()) {
        proxy.insert("client-fingerprint".into(), json!(f));
    }
    if let Some(pbk) = get("pbk").filter(|s| !s.is_empty()) {
        let mut r = Map::new();
        r.insert("public-key".into(), json!(pbk));
        if let Some(sid) = get("sid") {
            r.insert("short-id".into(), json!(sid));
        }
        proxy.insert("reality-opts".into(), Value::Object(r));
    }
    Some(Value::Object(proxy))
}

fn value_as_u64(v: Option<&Value>, default: u64) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
        _ => default,
    }
}

fn value_as_str(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_vmess_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let body = raw["vmess://".len()..].trim();
    let payload_part = body.split('#').next().unwrap_or(body);
    if let Some(decoded) = try_decode_base64_utf8(payload_part) {
        if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(decoded.trim()) {
            let server = value_as_str(obj.get("add"))?;
            let uuid = value_as_str(obj.get("id"))?;
            let port = value_as_u64(obj.get("port"), 443);
            let raw_name = value_as_str(obj.get("ps")).unwrap_or_else(|| server.clone());
            let name = make_unique_proxy_name(&raw_name, used_names);
            let tls_on = value_as_str(obj.get("tls"))
                .map(|s| s.eq_ignore_ascii_case("tls"))
                .unwrap_or(false);
            let aid = value_as_u64(obj.get("aid"), 0);
            let scy = value_as_str(obj.get("scy")).unwrap_or_else(|| "auto".into());
            let net = value_as_str(obj.get("net")).unwrap_or_else(|| "tcp".into());
            let host = value_as_str(obj.get("host"));
            let path = value_as_str(obj.get("path"));
            let htype = value_as_str(obj.get("type"));

            let mut proxy = Map::new();
            proxy.insert("name".into(), json!(name));
            proxy.insert("type".into(), json!("vmess"));
            proxy.insert("server".into(), json!(server));
            proxy.insert("port".into(), json!(port));
            proxy.insert("uuid".into(), json!(uuid));
            proxy.insert("alterId".into(), json!(aid));
            proxy.insert("cipher".into(), json!(scy));
            proxy.insert("udp".into(), json!(true));
            insert_transport_fields(
                &mut proxy,
                &net,
                host.as_deref(),
                path.as_deref(),
                None,
                None,
                htype.as_deref(),
            );
            if tls_on {
                proxy.insert("tls".into(), json!(true));
                if let Some(sni) = value_as_str(obj.get("sni")).or(host) {
                    proxy.insert("servername".into(), json!(sni));
                }
                if let Some(fp) = value_as_str(obj.get("fp")) {
                    proxy.insert("client-fingerprint".into(), json!(fp));
                }
                if let Some(alpn_s) = value_as_str(obj.get("alpn")) {
                    if let Some(a) = parse_comma_list(Some(&alpn_s)) {
                        proxy.insert("alpn".into(), json!(a));
                    }
                }
            }
            return Some(Value::Object(proxy));
        }
    }

    let u = Url::parse(raw).ok()?;
    let server = u.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    let port = u.port().unwrap_or(443);
    let uuid = safe_decode_uri_component(u.username());
    if server.is_empty() || uuid.is_empty() {
        return None;
    }
    let q = query_map(&u);
    let get = |k: &str| q.get(k).map(String::as_str);
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| server.clone());
    let name = make_unique_proxy_name(&raw_name, used_names);

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(name));
    proxy.insert("type".into(), json!("vmess"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("uuid".into(), json!(uuid));
    proxy.insert("alterId".into(), json!(0));
    proxy.insert("cipher".into(), json!(get("encryption").unwrap_or("auto")));
    proxy.insert("udp".into(), json!(true));
    insert_transport_fields(
        &mut proxy,
        get("type").unwrap_or("tcp"),
        get("host"),
        get("path"),
        get("serviceName"),
        None,
        None,
    );
    if get("security") == Some("tls") {
        proxy.insert("tls".into(), json!(true));
        if let Some(s) = get("sni").filter(|s| !s.is_empty()) {
            proxy.insert("servername".into(), json!(s));
        }
        if let Some(f) = get("fp").filter(|s| !s.is_empty()) {
            proxy.insert("client-fingerprint".into(), json!(f));
        }
    }
    Some(Value::Object(proxy))
}

fn parse_ss_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let u = Url::parse(raw).ok()?;
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .unwrap_or_default();
    let mut server = u
        .host_str()
        .unwrap_or("")
        .trim_matches(|c| c == '[' || c == ']')
        .to_string();
    let mut port = u.port().unwrap_or(0);
    let cipher;
    let password;

    if !server.is_empty() && port > 0 && !u.username().is_empty() {
        if let Some(pw) = u.password() {
            cipher = safe_decode_uri_component(u.username());
            password = safe_decode_uri_component(pw);
        } else {
            let user_decoded = try_decode_base64_utf8(&safe_decode_uri_component(u.username()))?;
            let (c, p) = user_decoded.split_once(':')?;
            cipher = c.to_string();
            password = p.to_string();
        }
    } else {
        let without_scheme = raw["ss://".len()..]
            .split('#')
            .next()?
            .split('?')
            .next()?;
        let decoded = try_decode_base64_utf8(without_scheme)?;
        let parsed = Url::parse(&format!("ss://{decoded}")).ok()?;
        server = parsed
            .host_str()?
            .trim_matches(|c| c == '[' || c == ']')
            .to_string();
        port = parsed.port()?;
        cipher = safe_decode_uri_component(parsed.username());
        password = safe_decode_uri_component(parsed.password().unwrap_or(""));
    }

    if server.is_empty() || port == 0 || cipher.is_empty() || password.is_empty() {
        return None;
    }
    let name_base = if raw_name.is_empty() { &server } else { &raw_name };
    let name = make_unique_proxy_name(name_base, used_names);
    Some(json!({
        "name": name,
        "type": "ss",
        "server": server,
        "port": port,
        "cipher": cipher,
        "password": password,
        "udp": true
    }))
}

fn parse_tuic_uri(raw: &str, used_names: &mut HashSet<String>) -> Option<Value> {
    let u = Url::parse(raw).ok()?;
    let server = u.host_str()?.trim_matches(|c| c == '[' || c == ']').to_string();
    let port = u.port().unwrap_or(443);
    let uuid = safe_decode_uri_component(u.username());
    let password = safe_decode_uri_component(u.password().unwrap_or(""));
    if server.is_empty() || uuid.is_empty() {
        return None;
    }
    let q = query_map(&u);
    let get = |k: &str| q.get(k).map(String::as_str);
    let raw_name = u
        .fragment()
        .map(safe_decode_uri_component)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| server.clone());
    let name = make_unique_proxy_name(&raw_name, used_names);

    let mut proxy = Map::new();
    proxy.insert("name".into(), json!(name));
    proxy.insert("type".into(), json!("tuic"));
    proxy.insert("server".into(), json!(server));
    proxy.insert("port".into(), json!(port));
    proxy.insert("uuid".into(), json!(uuid));
    if !password.is_empty() {
        proxy.insert("password".into(), json!(password));
    }
    if let Some(s) = get("sni").filter(|s| !s.is_empty()) {
        proxy.insert("sni".into(), json!(s));
    }
    if let Some(a) = parse_comma_list(get("alpn")) {
        proxy.insert("alpn".into(), json!(a));
    }
    if let Some(cc) = get("congestion_control").filter(|s| !s.is_empty()) {
        proxy.insert("congestion-controller".into(), json!(cc));
    }
    if let Some(um) = get("udp_relay_mode").filter(|s| !s.is_empty()) {
        proxy.insert("udp-relay-mode".into(), json!(um));
    }
    if parse_bool_param(get("allow_insecure").or_else(|| get("insecure"))) {
        proxy.insert("skip-cert-verify".into(), json!(true));
    }
    Some(Value::Object(proxy))
}

fn assemble_mihomo_yaml(proxies: Vec<Value>) -> String {
    let names: Vec<Value> = proxies
        .iter()
        .filter_map(|p| p.get("name").cloned())
        .collect();
    let mut proxy_group_list = Vec::with_capacity(names.len() + 2);
    proxy_group_list.push(json!("Auto"));
    proxy_group_list.extend(names.clone());
    proxy_group_list.push(json!("DIRECT"));

    let config = json!({
        "mixed-port": 7890,
        "allow-lan": false,
        "mode": "rule",
        "log-level": "info",
        "proxies": proxies,
        "proxy-groups": [
            {
                "name": "Proxy",
                "type": "select",
                "proxies": proxy_group_list
            },
            {
                "name": "Auto",
                "type": "url-test",
                "proxies": names,
                "url": "http://cp.cloudflare.com",
                "interval": 300
            }
        ],
        "rules": ["GEOIP,LAN,DIRECT,no-resolve", "MATCH,Proxy"]
    });
    stringify_json_to_yaml(&config).unwrap_or_default()
}

struct UriParseMeta {
    proxies: Vec<Value>,
    profile_title: Option<String>,
    subscription_userinfo: Option<String>,
    support_url: Option<String>,
    web_page_url: Option<String>,
}

fn parse_uri_lines(text: &str) -> UriParseMeta {
    let mut used_names = HashSet::new();
    let mut proxies = Vec::new();
    let mut profile_title = None;
    let mut subscription_userinfo = None;
    let mut support_url = None;
    let mut web_page_url = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') || line.starts_with("//") {
            let meta = line.trim_start_matches(['#', '/']).trim();
            if let Some((k, v)) = meta.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let val = v.trim();
                match key.as_str() {
                    "profile-title" => {
                        profile_title = if let Some(b64) = val.strip_prefix("base64:") {
                            try_decode_base64_utf8(b64).or_else(|| Some(val.to_string()))
                        } else {
                            Some(val.to_string())
                        };
                    }
                    "subscription-userinfo" => subscription_userinfo = Some(val.to_string()),
                    "support-url" => support_url = Some(val.to_string()),
                    "profile-web-page-url" => web_page_url = Some(val.to_string()),
                    _ => {}
                }
            }
            continue;
        }

        let lower = line.to_ascii_lowercase();
        let parsed = if lower.starts_with("vless://") {
            parse_vless_uri(line, &mut used_names)
        } else if lower.starts_with("hysteria2://") || lower.starts_with("hy2://") {
            parse_hysteria2_uri(line, &mut used_names)
        } else if lower.starts_with("trojan://") {
            parse_trojan_uri(line, &mut used_names)
        } else if lower.starts_with("vmess://") {
            parse_vmess_uri(line, &mut used_names)
        } else if lower.starts_with("ss://") {
            parse_ss_uri(line, &mut used_names)
        } else if lower.starts_with("tuic://") {
            parse_tuic_uri(line, &mut used_names)
        } else {
            None
        };
        if let Some(p) = parsed {
            proxies.push(p);
        }
    }

    UriParseMeta {
        proxies,
        profile_title,
        subscription_userinfo,
        support_url,
        web_page_url,
    }
}

fn convert_xray_or_singbox_json(raw_json: &str) -> Option<ConvertedSubscriptionResult> {
    let decoded: Value = serde_json::from_str(raw_json).ok()?;
    let entries = match decoded {
        Value::Array(arr) => arr,
        other @ Value::Object(_) => vec![other],
        _ => return None,
    };

    let mut used_names = HashSet::new();
    let mut proxies = Vec::new();
    let mut detected_type: Option<&'static str> = None;

    for entry in entries {
        let Some(map) = entry.as_object() else {
            continue;
        };
        let Some(outbounds) = map.get("outbounds").and_then(Value::as_array) else {
            continue;
        };
        let raw_title = value_as_str(map.get("remarks"))
            .or_else(|| value_as_str(map.get("ps")))
            .unwrap_or_else(|| "Server".to_string());

        for ob_raw in outbounds {
            let Some(ob) = ob_raw.as_object() else {
                continue;
            };

            // Sing-Box outbound shape
            if let (Some(sb_type), Some(server)) = (
                ob.get("type").and_then(Value::as_str),
                ob.get("server").and_then(Value::as_str),
            ) {
                detected_type = Some("singbox-json");
                let sb_lower = sb_type.to_ascii_lowercase();
                if matches!(
                    sb_lower.as_str(),
                    "direct" | "block" | "dns" | "selector" | "urltest"
                ) {
                    continue;
                }
                let tag = ob
                    .get("tag")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{sb_lower}-{server}"));
                let name = make_unique_proxy_name(&tag, &mut used_names);

                let tls = ob.get("tls").and_then(Value::as_object);
                let transport = ob.get("transport").and_then(Value::as_object);
                let reality = tls.and_then(|t| t.get("reality")).and_then(Value::as_object);
                let utls = tls.and_then(|t| t.get("utls")).and_then(Value::as_object);

                let t_net = transport
                    .and_then(|t| t.get("type"))
                    .and_then(Value::as_str)
                    .unwrap_or("tcp");
                let t_host = transport.and_then(|t| {
                    t.get("host")
                        .and_then(|h| {
                            h.as_array()
                                .and_then(|a| a.first())
                                .and_then(Value::as_str)
                                .or_else(|| h.as_str())
                        })
                        .or_else(|| {
                            t.get("headers")
                                .and_then(Value::as_object)
                                .and_then(|h| h.get("Host"))
                                .and_then(Value::as_str)
                        })
                });
                let t_path = transport.and_then(|t| t.get("path")).and_then(Value::as_str);
                let t_svc = transport
                    .and_then(|t| t.get("service_name"))
                    .and_then(Value::as_str);

                let mut proxy = Map::new();
                proxy.insert("name".into(), json!(name));
                proxy.insert(
                    "type".into(),
                    json!(if sb_lower == "shadowsocks" {
                        "ss"
                    } else {
                        sb_lower.as_str()
                    }),
                );
                proxy.insert("server".into(), json!(server));
                proxy.insert(
                    "port".into(),
                    json!(value_as_u64(ob.get("server_port"), 443)),
                );
                proxy.insert("udp".into(), json!(true));
                if sb_lower != "shadowsocks" {
                    insert_transport_fields(&mut proxy, t_net, t_host, t_path, t_svc, None, None);
                }

                if matches!(sb_lower.as_str(), "vless" | "vmess" | "tuic") {
                    if let Some(u) = ob.get("uuid") {
                        proxy.insert("uuid".into(), u.clone());
                    }
                    if let Some(f) = ob.get("flow") {
                        proxy.insert("flow".into(), f.clone());
                    }
                    if sb_lower == "vmess" {
                        proxy.insert("alterId".into(), json!(value_as_u64(ob.get("alter_id"), 0)));
                        proxy.insert(
                            "cipher".into(),
                            ob.get("security").cloned().unwrap_or_else(|| json!("auto")),
                        );
                    }
                }
                if let Some(pw) = ob.get("password") {
                    proxy.insert("password".into(), pw.clone());
                }
                if sb_lower == "shadowsocks" {
                    if let Some(m) = ob.get("method") {
                        proxy.insert("cipher".into(), m.clone());
                    }
                }
                if sb_lower == "hysteria2" {
                    if let Some(obfs_obj) = ob.get("obfs").and_then(Value::as_object) {
                        if let Some(t) = obfs_obj.get("type") {
                            proxy.insert("obfs".into(), t.clone());
                        }
                        if let Some(p) = obfs_obj.get("password") {
                            proxy.insert("obfs-password".into(), p.clone());
                        }
                    }
                }
                if let Some(tls_obj) = tls {
                    if tls_obj.get("enabled").and_then(Value::as_bool) == Some(true) {
                        proxy.insert("tls".into(), json!(true));
                        if let Some(sni) = tls_obj.get("server_name") {
                            if matches!(sb_lower.as_str(), "hysteria2" | "trojan" | "tuic") {
                                proxy.insert("sni".into(), sni.clone());
                            } else {
                                proxy.insert("servername".into(), sni.clone());
                            }
                        }
                        if tls_obj.get("insecure").and_then(Value::as_bool) == Some(true) {
                            proxy.insert("skip-cert-verify".into(), json!(true));
                        }
                        if let Some(alpn) = tls_obj.get("alpn").filter(|v| v.is_array()) {
                            proxy.insert("alpn".into(), alpn.clone());
                        }
                        if let Some(fp) = utls.and_then(|u| u.get("fingerprint")) {
                            proxy.insert("client-fingerprint".into(), fp.clone());
                        }
                        if let Some(r_obj) = reality {
                            if r_obj.get("enabled").and_then(Value::as_bool) == Some(true) {
                                if let Some(pbk) = r_obj.get("public_key") {
                                    let mut r = Map::new();
                                    r.insert("public-key".into(), json!(value_as_str(Some(pbk)).unwrap_or_default()));
                                    if let Some(sid) = r_obj.get("short_id") {
                                        r.insert("short-id".into(), json!(value_as_str(Some(sid)).unwrap_or_default()));
                                    }
                                    proxy.insert("reality-opts".into(), Value::Object(r));
                                }
                            }
                        }
                    }
                }
                proxies.push(Value::Object(proxy));
                continue;
            }

            // Xray-core outbound shape
            let Some(proto) = ob.get("protocol").and_then(Value::as_str) else {
                continue;
            };
            let proto_lower = proto.to_ascii_lowercase();
            if matches!(proto_lower.as_str(), "freedom" | "blackhole" | "dns") {
                continue;
            }
            detected_type = Some("xray-json");

            let settings = ob.get("settings").and_then(Value::as_object);
            let stream = ob.get("streamSettings").and_then(Value::as_object);
            let tls = stream.and_then(|s| s.get("tlsSettings")).and_then(Value::as_object);
            let reality = stream.and_then(|s| s.get("realitySettings")).and_then(Value::as_object);
            let ws = stream.and_then(|s| s.get("wsSettings")).and_then(Value::as_object);
            let grpc = stream.and_then(|s| s.get("grpcSettings")).and_then(Value::as_object);
            let xhttp = stream.and_then(|s| s.get("xhttpSettings")).and_then(Value::as_object);
            let sec = stream
                .and_then(|s| s.get("security"))
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_ascii_lowercase();

            let x_net = stream
                .and_then(|s| s.get("network"))
                .and_then(Value::as_str)
                .unwrap_or("tcp");
            let x_host = ws
                .and_then(|w| w.get("headers"))
                .and_then(Value::as_object)
                .and_then(|h| h.get("Host"))
                .and_then(Value::as_str)
                .or_else(|| xhttp.and_then(|x| x.get("host")).and_then(Value::as_str));
            let x_path = ws
                .and_then(|w| w.get("path"))
                .and_then(Value::as_str)
                .or_else(|| xhttp.and_then(|x| x.get("path")).and_then(Value::as_str));
            let x_svc = grpc.and_then(|g| g.get("serviceName")).and_then(Value::as_str);
            let x_mode = xhttp.and_then(|x| x.get("mode")).and_then(Value::as_str);

            match proto_lower.as_str() {
                "vless" | "vmess" => {
                    let vnext = settings
                        .and_then(|s| s.get("vnext"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_object);
                    let user = vnext
                        .and_then(|v| v.get("users"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_object);
                    let (Some(vn), Some(usr)) = (vnext, user) else {
                        continue;
                    };
                    let (Some(addr), Some(uuid)) = (vn.get("address"), usr.get("id")) else {
                        continue;
                    };
                    let name = make_unique_proxy_name(&raw_title, &mut used_names);
                    let mut proxy = Map::new();
                    proxy.insert("name".into(), json!(name));
                    proxy.insert("type".into(), json!(proto_lower));
                    proxy.insert("server".into(), addr.clone());
                    proxy.insert("port".into(), json!(value_as_u64(vn.get("port"), 443)));
                    proxy.insert("uuid".into(), uuid.clone());
                    proxy.insert("udp".into(), json!(true));
                    insert_transport_fields(&mut proxy, x_net, x_host, x_path, x_svc, x_mode, None);

                    if proto_lower == "vless" {
                        if let Some(f) = usr.get("flow").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                            proxy.insert("flow".into(), json!(f));
                        }
                    } else {
                        proxy.insert("alterId".into(), json!(value_as_u64(usr.get("alterId"), 0)));
                        proxy.insert(
                            "cipher".into(),
                            usr.get("security").cloned().unwrap_or_else(|| json!("auto")),
                        );
                    }
                    if sec == "tls" || sec == "reality" {
                        proxy.insert("tls".into(), json!(true));
                        if let Some(sni) = reality
                            .and_then(|r| r.get("serverName"))
                            .or_else(|| tls.and_then(|t| t.get("serverName")))
                        {
                            proxy.insert("servername".into(), sni.clone());
                        }
                        if let Some(fp) = reality
                            .and_then(|r| r.get("fingerprint"))
                            .or_else(|| tls.and_then(|t| t.get("fingerprint")))
                        {
                            proxy.insert("client-fingerprint".into(), fp.clone());
                        } else if sec == "reality" {
                            proxy.insert("client-fingerprint".into(), json!("chrome"));
                        }
                        if tls.and_then(|t| t.get("allowInsecure")).and_then(Value::as_bool) == Some(true)
                        {
                            proxy.insert("skip-cert-verify".into(), json!(true));
                        }
                        if sec == "reality" {
                            if let Some(pbk) = reality.and_then(|r| r.get("publicKey")) {
                                let mut r = Map::new();
                                r.insert("public-key".into(), json!(value_as_str(Some(pbk)).unwrap_or_default()));
                                if let Some(sid) = reality.and_then(|r| r.get("shortId")) {
                                    r.insert("short-id".into(), json!(value_as_str(Some(sid)).unwrap_or_default()));
                                }
                                proxy.insert("reality-opts".into(), Value::Object(r));
                            }
                        }
                    }
                    proxies.push(Value::Object(proxy));
                }
                "hysteria" => {
                    let hy = stream.and_then(|s| s.get("hysteriaSettings")).and_then(Value::as_object);
                    let (Some(addr), Some(auth)) = (
                        settings.and_then(|s| s.get("address")),
                        hy.and_then(|h| h.get("auth")),
                    ) else {
                        continue;
                    };
                    let name = make_unique_proxy_name(&raw_title, &mut used_names);
                    let mut proxy = Map::new();
                    proxy.insert("name".into(), json!(name));
                    proxy.insert("type".into(), json!("hysteria2"));
                    proxy.insert("server".into(), addr.clone());
                    proxy.insert(
                        "port".into(),
                        json!(value_as_u64(settings.and_then(|s| s.get("port")), 443)),
                    );
                    proxy.insert("password".into(), auth.clone());
                    if let Some(sni) = tls.and_then(|t| t.get("serverName")) {
                        proxy.insert("sni".into(), sni.clone());
                    }
                    if tls.and_then(|t| t.get("allowInsecure")).and_then(Value::as_bool) == Some(true)
                    {
                        proxy.insert("skip-cert-verify".into(), json!(true));
                    }
                    if let Some(udp_masks) = stream
                        .and_then(|s| s.get("finalmask"))
                        .and_then(Value::as_object)
                        .and_then(|f| f.get("udp"))
                        .and_then(Value::as_array)
                    {
                        for mask in udp_masks {
                            if mask.get("type").and_then(Value::as_str) == Some("salamander") {
                                if let Some(pw) = mask
                                    .get("settings")
                                    .and_then(|s| s.get("password"))
                                    .cloned()
                                {
                                    proxy.insert("obfs".into(), json!("salamander"));
                                    proxy.insert("obfs-password".into(), pw);
                                    break;
                                }
                            }
                        }
                    }
                    proxies.push(Value::Object(proxy));
                }
                "trojan" | "shadowsocks" => {
                    let server_obj = settings
                        .and_then(|s| s.get("servers"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_object);
                    let Some(sv) = server_obj else {
                        continue;
                    };
                    let (Some(addr), Some(pw)) = (sv.get("address"), sv.get("password")) else {
                        continue;
                    };
                    let name = make_unique_proxy_name(&raw_title, &mut used_names);
                    let mut proxy = Map::new();
                    proxy.insert("name".into(), json!(name));
                    proxy.insert(
                        "type".into(),
                        json!(if proto_lower == "shadowsocks" {
                            "ss"
                        } else {
                            "trojan"
                        }),
                    );
                    proxy.insert("server".into(), addr.clone());
                    proxy.insert("port".into(), json!(value_as_u64(sv.get("port"), 443)));
                    proxy.insert("password".into(), pw.clone());
                    proxy.insert("udp".into(), json!(true));
                    if proto_lower == "shadowsocks" {
                        proxy.insert(
                            "cipher".into(),
                            sv.get("method")
                                .cloned()
                                .unwrap_or_else(|| json!("aes-256-gcm")),
                        );
                    } else {
                        insert_transport_fields(&mut proxy, x_net, x_host, x_path, x_svc, x_mode, None);
                        if let Some(sni) = tls.and_then(|t| t.get("serverName")) {
                            proxy.insert("sni".into(), sni.clone());
                        }
                        if tls.and_then(|t| t.get("allowInsecure")).and_then(Value::as_bool)
                            == Some(true)
                        {
                            proxy.insert("skip-cert-verify".into(), json!(true));
                        }
                    }
                    proxies.push(Value::Object(proxy));
                }
                _ => {}
            }
        }
    }

    if proxies.is_empty() {
        return None;
    }
    let first_name = proxies[0]
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(ConvertedSubscriptionResult {
        yaml: assemble_mihomo_yaml(proxies),
        profile_title: None,
        subscription_userinfo: None,
        support_url: None,
        web_page_url: None,
        first_node_name: first_name,
        converted_from: detected_type.unwrap_or("xray-json"),
    })
}

pub fn normalize_subscription_to_mihomo_yaml(raw_data: &str) -> Option<ConvertedSubscriptionResult> {
    let cleaned = raw_data.trim_start_matches('\u{FEFF}').trim();
    if cleaned.is_empty() {
        return None;
    }
    let lower_head: String = cleaned.chars().take(32).collect::<String>().to_ascii_lowercase();
    if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
        return None;
    }

    if cleaned.starts_with('{') || cleaned.starts_with('[') {
        if let Some(res) = convert_xray_or_singbox_json(cleaned) {
            return Some(res);
        }
    }

    if !is_share_link_input(cleaned) {
        if let Ok(parsed) = parse_yaml_to_json(cleaned) {
            if is_valid_mihomo_object(&parsed) {
                return Some(ConvertedSubscriptionResult {
                    yaml: cleaned.to_string(),
                    profile_title: None,
                    subscription_userinfo: None,
                    support_url: None,
                    web_page_url: None,
                    first_node_name: None,
                    converted_from: "yaml",
                });
            }
        }
    }

    if let Some(decoded) = try_decode_base64_utf8(cleaned) {
        let dec_trim = decoded.trim();
        if dec_trim.starts_with('{') || dec_trim.starts_with('[') {
            if let Some(res) = convert_xray_or_singbox_json(dec_trim) {
                return Some(res);
            }
        }
        if !is_share_link_input(dec_trim) {
            if let Ok(parsed) = parse_yaml_to_json(dec_trim) {
                if is_valid_mihomo_object(&parsed) {
                    return Some(ConvertedSubscriptionResult {
                        yaml: dec_trim.to_string(),
                        profile_title: None,
                        subscription_userinfo: None,
                        support_url: None,
                        web_page_url: None,
                        first_node_name: None,
                        converted_from: "yaml",
                    });
                }
            }
        }
        let uri_res = parse_uri_lines(dec_trim);
        if !uri_res.proxies.is_empty() {
            let first = uri_res.proxies[0]
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string);
            return Some(ConvertedSubscriptionResult {
                yaml: assemble_mihomo_yaml(uri_res.proxies),
                profile_title: uri_res.profile_title,
                subscription_userinfo: uri_res.subscription_userinfo,
                support_url: uri_res.support_url,
                web_page_url: uri_res.web_page_url,
                first_node_name: first,
                converted_from: "uri",
            });
        }
    }

    let raw_uri_res = parse_uri_lines(cleaned);
    if !raw_uri_res.proxies.is_empty() {
        let first = raw_uri_res.proxies[0]
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string);
        return Some(ConvertedSubscriptionResult {
            yaml: assemble_mihomo_yaml(raw_uri_res.proxies),
            profile_title: raw_uri_res.profile_title,
            subscription_userinfo: raw_uri_res.subscription_userinfo,
            support_url: raw_uri_res.support_url,
            web_page_url: raw_uri_res.web_page_url,
            first_node_name: first,
            converted_from: "uri",
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_base64_vless_and_hy2() {
        let sub = "#profile-title: base64:TnVsbGV4\n\
                   #subscription-userinfo: upload=10; download=20; total=100; expire=1900000000\n\
                   vless://11111111-1111-4111-8111-111111111111@de.example.com:443?security=reality&pbk=bG9rS2V5U2FtcGxlVGVzdEtleTEyMzQ1Njc4OTAxMjM&sid=a1b2&sni=www.microsoft.com&fp=chrome#DE\n\
                   hy2://pass@nl.example.com:8443?insecure=1&obfs=salamander&obfs-password=opw#NL";
        let b64 = STANDARD.encode(sub.as_bytes());
        let res = normalize_subscription_to_mihomo_yaml(&b64).expect("should convert");
        assert_eq!(res.converted_from, "uri");
        assert_eq!(res.profile_title.as_deref(), Some("Nullex"));
        assert!(res.yaml.contains("type: vless"));
        assert!(res.yaml.contains("type: hysteria2"));
        assert!(res.yaml.contains("salamander"));
    }

    #[test]
    fn converts_xray_and_singbox_json() {
        let xray = r#"[{"remarks":"XrayNode","outbounds":[{"protocol":"vless","settings":{"vnext":[{"address":"x.example.com","port":443,"users":[{"id":"22222222-2222-4222-8222-222222222222"}]}]},"streamSettings":{"network":"xhttp","security":"reality","realitySettings":{"serverName":"dl.google.com","publicKey":"bG9rS2V5U2FtcGxlVGVzdEtleTEyMzQ1Njc4OTAxMjM","shortId":"12ab"},"xhttpSettings":{"path":"/xhttp","mode":"auto"}}}]}]"#;
        let rx = normalize_subscription_to_mihomo_yaml(xray).expect("xray should convert");
        assert_eq!(rx.converted_from, "xray-json");
        assert!(rx.yaml.contains("network: xhttp"));

        let sb = r#"{"outbounds":[{"type":"hysteria2","tag":"SBHy2","server":"s.example.com","server_port":8443,"password":"pw","obfs":{"type":"salamander","password":"op"},"tls":{"enabled":true,"insecure":true,"server_name":"s.example.com"}}]}"#;
        let rs = normalize_subscription_to_mihomo_yaml(sb).expect("singbox should convert");
        assert_eq!(rs.converted_from, "singbox-json");
        assert!(rs.yaml.contains("SBHy2"));
    }

    #[test]
    fn rejects_html() {
        assert!(normalize_subscription_to_mihomo_yaml("<!DOCTYPE html><html></html>").is_none());
    }
}
