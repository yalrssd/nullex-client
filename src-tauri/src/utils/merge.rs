use serde_json::{Map, Value};

fn trim_wrap(s: &str) -> &str {
    if s.starts_with('<') && s.ends_with('>') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Performs a deep merge of JSON values supporting `<key>` unwrap and `key!` force-replace semantics.
pub fn deep_merge(target: &mut Value, other: &Value) {
    let (Value::Object(t_map), Value::Object(o_map)) = (target, other) else {
        return;
    };

    for (raw_key, val) in o_map {
        if val.is_object() {
            if let Some(stripped) = raw_key.strip_suffix('!') {
                let k = trim_wrap(stripped).to_string();
                t_map.insert(k, val.clone());
            } else {
                let k = trim_wrap(raw_key).to_string();
                let entry = t_map
                    .entry(k)
                    .or_insert_with(|| Value::Object(Map::new()));
                if !entry.is_object() {
                    *entry = Value::Object(Map::new());
                }
                deep_merge(entry, val);
            }
        } else if val.is_array() {
            let k = trim_wrap(raw_key).to_string();
            t_map.insert(k, val.clone());
        } else {
            t_map.insert(raw_key.clone(), val.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recursively_merges_objects_and_replaces_arrays() {
        let mut target = json!({
            "dns": {"enable": false, "nameserver": ["1.1.1.1"]},
            "rules": ["MATCH,DIRECT"],
            "mode": "rule"
        });
        let other = json!({
            "dns": {"enable": true},
            "rules": ["MATCH,PROXY"],
            "mixed-port": 7897
        });

        deep_merge(&mut target, &other);

        assert_eq!(target["dns"]["enable"], json!(true));
        assert_eq!(target["dns"]["nameserver"], json!(["1.1.1.1"]));
        assert_eq!(target["rules"], json!(["MATCH,PROXY"]));
        assert_eq!(target["mixed-port"], json!(7897));
    }

    #[test]
    fn supports_wrapped_keys_and_force_replace() {
        let mut target = json!({
            "tun": {"enable": false, "stack": "system"},
            "dns": {"enable": false}
        });
        let other = json!({
            "<tun>": {"enable": true},
            "<dns>!": {"enable": true}
        });

        deep_merge(&mut target, &other);

        assert_eq!(target["tun"]["enable"], json!(true));
        assert_eq!(target["tun"]["stack"], json!("system"));
        assert_eq!(target["dns"], json!({"enable": true}));
    }

    #[test]
    fn replaces_scalar_target_before_recursive_merge() {
        let mut target = json!({"tun": false});
        let other = json!({"tun": {"enable": true}});

        deep_merge(&mut target, &other);

        assert_eq!(target["tun"], json!({"enable": true}));
    }
}
