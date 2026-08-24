//! Secret masking and redaction — "secrets are masked by default".
//!
//! The single choke point through which any serialized resource passes before reaching a
//! frontend, the MCP surface, or an audit log. Kubernetes `Secret` values, `stringData`,
//! and any field whose key looks sensitive (password, token, key, credential, private_key,
//! bearer, etc.) are replaced with a `[REDACTED]` marker **before** serialization, so a
//! plaintext secret can never leak through `kaptein describe`, the MCP `describe` tool, or
//! a diagnostic summary.
//!
//! This is a hard rule from AGENTS.md/SECURITY.md ("secrets are masked by default"), and it
//! must run **in `kaptein-core`** — the layer every frontend and the MCP server already
//! route through — so no caller can forget it. Redaction is idempotent and applied to the
//! `DynamicObject` before `serde_yaml` sees it.

use kube::core::DynamicObject;

/// The marker that replaces a masked value.
pub const REDACTED: &str = "[REDACTED]";

/// Field names (case-insensitive) whose values are always masked, regardless of resource
/// kind. These are the common secret-shaped keys across ConfigMaps, Deployments (env),
/// CRDs, and tool configs.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "secret_key",
    "secret",
    "client_secret",
    "private_key",
    "credentials",
    "credential",
    "bearer",
    "authorization",
    "auth_token",
    "cookie",
    "session",
    "jwt",
    "ssh_key",
    "tls.key",
    "ca.crt",
];

/// Whether a resource kind holds secret data by definition.
fn is_secret_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("Secret")
        || kind.eq_ignore_ascii_case("ExternalSecret")
        || kind.eq_ignore_ascii_case("ClusterSecretStore")
        || kind.eq_ignore_ascii_case("SecretStore")
}

/// Redact a `DynamicObject` in place: masks Secret `data`/`stringData` and any
/// sensitive-named field, recursively, before it is serialized. Idempotent.
pub fn redact_object(obj: &mut DynamicObject) {
    let kind = obj
        .types
        .as_ref()
        .map(|t| t.kind.as_str())
        .unwrap_or_default();

    // A `Secret`'s `data` and `stringData` are the whole point of the object — mask every
    // value in them.
    if is_secret_kind(kind) {
        if let Some(data) = obj.data.get_mut("data").and_then(|d| d.as_object_mut()) {
            for value in data.values_mut() {
                *value = serde_json::Value::String(REDACTED.to_string());
            }
        }
        if let Some(data) = obj
            .data
            .get_mut("stringData")
            .and_then(|d| d.as_object_mut())
        {
            for value in data.values_mut() {
                *value = serde_json::Value::String(REDACTED.to_string());
            }
        }
    }

    // Recursively mask any sensitive-named key anywhere in the object.
    redact_value(&mut obj.data, kind);
}

fn redact_value(value: &mut serde_json::Value, _kind: &str) {
    match value {
        serde_json::Value::Object(map) => {
            // Env-var style pairs: `{"name": "DB_PASSWORD", "value": "..."}` — mask the
            // `value` when the paired `name` is sensitive.
            if let Some(name) = map.get("name").and_then(|n| n.as_str())
                && is_sensitive_key(name)
                && let Some(val) = map.get_mut("value")
            {
                *val = serde_json::Value::String(REDACTED.to_string());
            }
            for (key, val) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *val = serde_json::Value::String(REDACTED.to_string());
                } else {
                    redact_value(val, _kind);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_value(item, _kind);
            }
        }
        _ => {}
    }
}

/// Whether a key name (lowercased) should be masked. Handles nested paths like
/// `tls.key` and dotted env-var style names.
fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEYS.iter().any(|s| {
        lower == *s
            || lower.ends_with(&format!(".{s}"))
            || lower.ends_with(&format!("_{s}"))
            || lower.ends_with(&format!("-{s}"))
            || lower.starts_with(&format!("{s}."))
            || lower.contains(&format!(".{s}."))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use serde_json::json;

    fn obj(kind: &str, data: serde_json::Value) -> DynamicObject {
        DynamicObject {
            types: Some(kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: kind.into(),
            }),
            metadata: ObjectMeta::default(),
            data,
        }
    }

    #[test]
    fn secret_data_is_fully_masked() {
        let mut o = obj(
            "Secret",
            json!({
                "data": { "username": "dXNlcg==", "password": "c2VjcmV0" },
                "stringData": { "api_key": "plaintext-key" }
            }),
        );
        redact_object(&mut o);
        let s = serde_json::to_string(&o.data).unwrap();
        assert!(!s.contains("dXNlcg=="));
        assert!(!s.contains("c2VjcmV0"));
        assert!(!s.contains("plaintext-key"));
        assert!(s.contains(REDACTED));
    }

    #[test]
    fn sensitive_keys_are_masked_outside_secrets() {
        let mut o = obj(
            "Deployment",
            json!({
                "spec": { "template": { "spec": { "containers": [{
                    "name": "app",
                    "env": [{"name": "DB_PASSWORD", "value": "hunter2"}]
                }]}}}
            }),
        );
        redact_object(&mut o);
        let s = serde_json::to_string(&o.data).unwrap();
        assert!(!s.contains("hunter2"));
    }

    #[test]
    fn non_sensitive_data_is_untouched() {
        let mut o = obj("ConfigMap", json!({ "data": { "mode": "production" } }));
        redact_object(&mut o);
        let s = serde_json::to_string(&o.data).unwrap();
        assert!(s.contains("production"));
        assert!(!s.contains(REDACTED));
    }

    #[test]
    fn redaction_is_idempotent() {
        let mut o = obj("Secret", json!({ "data": { "k": "v" } }));
        redact_object(&mut o);
        redact_object(&mut o);
        let s = serde_json::to_string(&o.data).unwrap();
        assert!(!s.contains("\"v\""));
    }
}
