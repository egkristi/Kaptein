//! Contract-version enforcement (see `docs/versioning.md`).
//!
//! Kaptein has three independently-versioned contracts — the **MCP tool schema**, the
//! **lens** (view-definition) schema, and the **WIT** worlds. Each carries its own
//! `api_version`/schema version, bumped independently on a breaking change to *that*
//! contract. A release must **refuse to load a plugin, lens, or MCP client whose
//! version it does not support**, with a clear migration error — never silently break.
//!
//! This module is wasm-pure (no `kube`/`tokio`), so the browser UI and the headless
//! agent share the exact same compatibility rule as the native frontends.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A contract's `api_version`/schema version, e.g. `v1` or `2`.
///
/// Parsed from strings like `"1"`, `"v1"`, `"1.2"`, or `"v1.2"`. Semantics follow
/// `docs/versioning.md`: the **major** identifies a breaking contract generation;
/// additive changes bump the **minor** without breaking compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl ApiVersion {
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minor == 0 {
            write!(f, "v{}", self.major)
        } else {
            write!(f, "v{}.{}", self.major, self.minor)
        }
    }
}

/// Parse an `api_version` string such as `"1"`, `"v1"`, `"1.2"`, or `"v1.2"`.
///
/// Returns `None` for a malformed or empty version.
pub fn parse_api_version(s: &str) -> Option<ApiVersion> {
    let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next().map(|p| p.parse().unwrap_or(0)).unwrap_or(0);
    if parts.next().is_some() {
        return None; // too many components
    }
    Some(ApiVersion { major, minor })
}

/// Whether a client/plugin/lens requesting `requested` is supported by a release
/// implementing `supported`.
///
/// Compatibility is **same major**: additive (minor) changes are compatible, a major
/// bump is a breaking contract change and is refused with a migration error.
pub fn is_compatible(supported: ApiVersion, requested: ApiVersion) -> bool {
    requested.major == supported.major
}

/// The Kaptein MCP tool-schema contract version. Bump the **major** on an incompatible
/// tool change (a renamed/removed tool or a changed required argument), per
/// `docs/versioning.md`; bump the **minor** for an additive tool or optional argument.
pub const MCP_API_VERSION: ApiVersion = ApiVersion::new(1, 0);

/// The `_meta` key under which the MCP client declares the contract version it speaks.
pub const MCP_VERSION_META_KEY: &str = "io.kaptein/apiVersion";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_forms() {
        assert_eq!(parse_api_version("1"), Some(ApiVersion::new(1, 0)));
        assert_eq!(parse_api_version("v1"), Some(ApiVersion::new(1, 0)));
        assert_eq!(parse_api_version("1.2"), Some(ApiVersion::new(1, 2)));
        assert_eq!(parse_api_version("v1.2"), Some(ApiVersion::new(1, 2)));
        assert_eq!(parse_api_version("  v2  "), Some(ApiVersion::new(2, 0)));
    }

    #[test]
    fn parse_rejects_malformed() {
        assert_eq!(parse_api_version(""), None);
        assert_eq!(parse_api_version("v"), None);
        assert_eq!(parse_api_version("abc"), None);
        assert_eq!(parse_api_version("1.2.3"), None);
        assert_eq!(parse_api_version("v1.2.3"), None);
    }

    #[test]
    fn same_major_is_compatible() {
        assert!(is_compatible(ApiVersion::new(1, 0), ApiVersion::new(1, 9)));
        assert!(is_compatible(ApiVersion::new(1, 9), ApiVersion::new(1, 0)));
    }

    #[test]
    fn different_major_is_incompatible() {
        assert!(!is_compatible(ApiVersion::new(1, 0), ApiVersion::new(2, 0)));
        assert!(!is_compatible(ApiVersion::new(2, 0), ApiVersion::new(1, 0)));
    }

    #[test]
    fn display_omits_zero_minor() {
        assert_eq!(ApiVersion::new(1, 0).to_string(), "v1");
        assert_eq!(ApiVersion::new(1, 2).to_string(), "v1.2");
    }
}
