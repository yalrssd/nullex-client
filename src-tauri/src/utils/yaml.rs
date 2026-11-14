use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use serde_yaml_ng::{Mapping, Value as YamlValue};

/// Parse YAML into JSON Value safely while normalizing Clash fields (such as unquoted numeric `short-id`).
pub fn parse_yaml_to_json(raw: &str) -> Result<JsonValue, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(JsonValue::Object(JsonMap::new()));
    }
    let val: YamlValue =
        serde_yaml_ng::from_str(trimmed).map_err(|e| format!("YAML parse error: {e}"))?;
    let mut json = yaml_to_json(val);
    normalize_short_ids(&mut json);
    Ok(json)
}

pub fn stringify_json_to_yaml(val: &JsonValue) -> Result<String, String> {
    let yaml_val = json_to_yaml(val.clone());
    serde_yaml_ng::to_string(&yaml_val).map_err(|e| format!("YAML serialize error: {e}"))
}

fn normalize_short_ids(val: &mut JsonValue) {
    match val {
        JsonValue::Object(map) => {
            if let Some(v) = map.get_mut("short-id") {
                match v {
                    JsonValue::Number(n) => {
                        *v = JsonValue::String(n.to_string());
                    }
                    JsonValue::Null => {
                        *v = JsonValue::String(String::new());
                    }
                    _ => {}
                }
            }
            for child in map.values_mut() {
                normalize_short_ids(child);
            }
        }
        JsonValue::Array(arr) => {
            for item in arr {
                normalize_short_ids(item);
            }
        }
        _ => {}
    }
}

pub fn yaml_to_json(val: YamlValue) -> JsonValue {
    match val {
        YamlValue::Null => JsonValue::Null,
        YamlValue::Bool(b) => JsonValue::Bool(b),
        YamlValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                JsonValue::Number(JsonNumber::from(i))
            } else if let Some(u) = n.as_u64() {
                JsonValue::Number(JsonNumber::from(u))
            } else if let Some(f) = n.as_f64() {
                JsonNumber::from_f64(f)
                    .map(JsonValue::Number)
                    .unwrap_or(JsonValue::Null)
            } else {
                JsonValue::Null
            }
        }
        YamlValue::String(s) => JsonValue::String(s),
        YamlValue::Sequence(seq) => JsonValue::Array(seq.into_iter().map(yaml_to_json).collect()),
        YamlValue::Mapping(map) => {
            let mut out = JsonMap::with_capacity(map.len());
            for (k, v) in map {
                let key_str = match k {
                    YamlValue::String(s) => s,
                    YamlValue::Number(n) => n.to_string(),
                    YamlValue::Bool(b) => b.to_string(),
                    other => serde_yaml_ng::to_string(&other).unwrap_or_default().trim().to_string(),
                };
                out.insert(key_str, yaml_to_json(v));
            }
            JsonValue::Object(out)
        }
        YamlValue::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

pub fn json_to_yaml(val: JsonValue) -> YamlValue {
    match val {
        JsonValue::Null => YamlValue::Null,
        JsonValue::Bool(b) => YamlValue::Bool(b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                YamlValue::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                YamlValue::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                YamlValue::Number(f.into())
            } else {
                YamlValue::Null
            }
        }
        JsonValue::String(s) => YamlValue::String(s),
        JsonValue::Array(arr) => YamlValue::Sequence(arr.into_iter().map(json_to_yaml).collect()),
        JsonValue::Object(map) => {
            let mut out = Mapping::with_capacity(map.len());
            for (k, v) in map {
                out.insert(YamlValue::String(k), json_to_yaml(v));
            }
            YamlValue::Mapping(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_empty_yaml_as_empty_object() {
        assert_eq!(parse_yaml_to_json("  \n").unwrap(), json!({}));
    }

    #[test]
    fn normalizes_numeric_and_null_short_ids_recursively() {
        let value = parse_yaml_to_json(
            "proxies:\n  - short-id: 1234\n  - nested:\n      short-id: null\n  - short-id: abc",
        )
        .unwrap();

        assert_eq!(value["proxies"][0]["short-id"], json!("1234"));
        assert_eq!(value["proxies"][1]["nested"]["short-id"], json!(""));
        assert_eq!(value["proxies"][2]["short-id"], json!("abc"));
    }

    #[test]
    fn round_trips_json_through_yaml() {
        let original = json!({
            "mode": "rule",
            "mixed-port": 7897,
            "tun": {"enable": false, "dns-hijack": ["any:53"]},
            "rules": ["MATCH,DIRECT"]
        });

        let yaml = stringify_json_to_yaml(&original).unwrap();
        let parsed = parse_yaml_to_json(&yaml).unwrap();
        assert_eq!(parsed, original);
    }
}
