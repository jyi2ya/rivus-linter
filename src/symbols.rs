use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CrateName(String);

impl CrateName {
    /// Normalize a Cargo manifest crate name into its rustc def-path form.
    pub fn rvs_from_manifest_name(name: &str) -> Self {
        Self(name.replace('-', "_"))
    }

    /// Borrow the normalized crate name as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }

    /// Build the canonical `crate_name::` def-path prefix.
    pub fn rvs_prefix(&self) -> DefPathPrefix {
        DefPathPrefix(format!("{}::", self.0))
    }
}

impl fmt::Display for CrateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_as_str())
    }
}

impl AsRef<str> for CrateName {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for CrateName {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

impl From<&str> for CrateName {
    fn from(value: &str) -> Self {
        Self::rvs_from_manifest_name(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DefPathPrefix(String);

impl DefPathPrefix {
    /// Borrow the def-path prefix as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }

    /// Append a function name onto the prefix to form a complete def-path.
    pub fn rvs_join_name(&self, fn_name: &FnName) -> DefPath {
        DefPath(format!("{}{fn_name}", self.0))
    }
}

impl AsRef<str> for DefPathPrefix {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for DefPathPrefix {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FnName(String);

impl FnName {
    /// Wrap a bare function name.
    pub fn rvs_new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Borrow the function name as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FnName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_as_str())
    }
}

impl AsRef<str> for FnName {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for FnName {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

impl From<&str> for FnName {
    fn from(value: &str) -> Self {
        Self::rvs_new(value)
    }
}

impl From<String> for FnName {
    fn from(value: String) -> Self {
        Self::rvs_new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DefPath(String);

/// Borrowed semantic view of a serialized trait-implementation method path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TraitMethodIdentity<'a> {
    implementation_path: &'a str,
    method_name: &'a str,
    trait_path: &'a str,
}

impl<'a> TraitMethodIdentity<'a> {
    /// Parse the stable `implementation_path@trait_path` artifact representation.
    pub(crate) fn rvs_parse(path: &'a str) -> Option<Self> {
        let (implementation_path, trait_path) = path.split_once('@')?;
        if implementation_path.is_empty() || trait_path.is_empty() || trait_path.contains('@') {
            return None;
        }
        let method_name = implementation_path.rsplit("::").next()?;
        if method_name.is_empty() {
            return None;
        }
        Some(Self {
            implementation_path,
            method_name,
            trait_path,
        })
    }

    /// Return the bare implementation method name.
    pub(crate) fn rvs_method_name(self) -> &'a str {
        self.method_name
    }

    /// Build the canonical trait declaration path represented by this implementation.
    pub(crate) fn rvs_trait_method_path(self) -> DefPath {
        DefPath::rvs_new(format!("{}::{}", self.trait_path, self.method_name))
    }

    /// Build the canonical implementation-index key.
    pub(crate) fn rvs_lookup_key(self) -> TraitMethodKey {
        TraitMethodKey::rvs_new(format!("{}@{}", self.method_name, self.trait_path))
    }
}

/// Canonical `method_name@trait_path` key used to group trait implementations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TraitMethodKey(String);

impl TraitMethodKey {
    fn rvs_new(key: String) -> Self {
        Self(key)
    }

    /// Build an implementation-index key from a trait declaration path.
    pub(crate) fn rvs_from_trait_method(path: &DefPath) -> Option<Self> {
        if path.rvs_trait_method_identity().is_some() {
            return None;
        }
        let (trait_path, method_name) = path.rvs_as_str().rsplit_once("::")?;
        if trait_path.is_empty() || method_name.is_empty() {
            return None;
        }
        Some(Self::rvs_new(format!("{method_name}@{trait_path}")))
    }

    /// Borrow the canonical lookup key as `&str`.
    pub(crate) fn rvs_as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for TraitMethodKey {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

/// Return the function segment before any trait-implementation suffix.
pub(crate) fn rvs_function_name_segment(name: &str) -> &str {
    TraitMethodIdentity::rvs_parse(name).map_or_else(
        || {
            let method_path = name.split_once('@').map_or(name, |(method, _)| method);
            method_path.rsplit("::").next().unwrap_or(method_path)
        },
        TraitMethodIdentity::rvs_method_name,
    )
}

impl DefPath {
    /// Wrap a canonical def-path string.
    pub fn rvs_new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// Borrow the def-path as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }

    /// Return the last function-name segment of the def-path.
    pub fn rvs_fn_name(&self) -> FnName {
        FnName::rvs_new(rvs_function_name_segment(&self.0))
    }

    /// Parse this path as a serialized trait-implementation method identity.
    pub(crate) fn rvs_trait_method_identity(&self) -> Option<TraitMethodIdentity<'_>> {
        TraitMethodIdentity::rvs_parse(&self.0)
    }

    /// Return whether this def-path belongs to the given crate prefix.
    pub fn rvs_starts_with(&self, prefix: &DefPathPrefix) -> bool {
        self.0.starts_with(prefix.rvs_as_str())
    }

    /// Remove a local crate prefix and return the workspace-relative function path.
    pub fn rvs_strip_prefix(&self, prefix: &DefPathPrefix) -> Option<RelativeFnPath> {
        self.0
            .strip_prefix(prefix.rvs_as_str())
            .map(|path| RelativeFnPath::rvs_new(path.to_string()))
    }

    /// Return whether the def-path contains a substring.
    pub fn rvs_contains(&self, needle: &str) -> bool {
        self.0.contains(needle)
    }
}

impl fmt::Display for DefPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_as_str())
    }
}

impl AsRef<str> for DefPath {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for DefPath {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

impl From<&str> for DefPath {
    fn from(value: &str) -> Self {
        Self::rvs_new(value)
    }
}

impl From<String> for DefPath {
    fn from(value: String) -> Self {
        Self::rvs_new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelativeFnPath(String);

impl RelativeFnPath {
    /// Wrap a workspace-local relative function path.
    pub fn rvs_new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// Borrow the relative function path as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }

    /// Return the last function-name segment of the relative path.
    pub fn rvs_fn_name(&self) -> FnName {
        FnName::rvs_new(rvs_function_name_segment(&self.0))
    }
}

impl fmt::Display for RelativeFnPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_as_str())
    }
}

impl AsRef<str> for RelativeFnPath {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for RelativeFnPath {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

impl From<&str> for RelativeFnPath {
    fn from(value: &str) -> Self {
        Self::rvs_new(value)
    }
}

impl From<String> for RelativeFnPath {
    fn from(value: String) -> Self {
        Self::rvs_new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapsMapKey(String);

impl CapsMapKey {
    /// Wrap a capsmap key string.
    pub fn rvs_new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// Borrow the capsmap key as `&str`.
    pub fn rvs_as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapsMapKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_as_str())
    }
}

impl AsRef<str> for CapsMapKey {
    fn as_ref(&self) -> &str {
        self.rvs_as_str()
    }
}

impl Borrow<str> for CapsMapKey {
    fn borrow(&self) -> &str {
        self.rvs_as_str()
    }
}

impl From<&str> for CapsMapKey {
    fn from(value: &str) -> Self {
        Self::rvs_new(value)
    }
}

impl From<String> for CapsMapKey {
    fn from(value: String) -> Self {
        Self::rvs_new(value)
    }
}

impl From<DefPath> for CapsMapKey {
    fn from(value: DefPath) -> Self {
        Self(value.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::rvs_snapshot_BIS;

    #[test]
    fn test_20260702_crate_name_normalizes_manifest_name() {
        let crate_name = CrateName::rvs_from_manifest_name("cargo-rivus");
        let output = format!(
            "name={}\nprefix={}",
            crate_name,
            crate_name.rvs_prefix().rvs_as_str()
        );
        rvs_snapshot_BIS("test_20260702_crate_name_normalizes_manifest_name", &output);
        assert_eq!(crate_name.rvs_as_str(), "cargo_rivus");
        assert_eq!(crate_name.rvs_prefix().rvs_as_str(), "cargo_rivus::");
    }

    #[test]
    fn test_20260702_def_path_strips_local_prefix() {
        let def_path = DefPath::rvs_new("cargo_rivus::cli::main");
        let prefix = CrateName::rvs_from_manifest_name("cargo-rivus").rvs_prefix();
        let root_main = prefix.rvs_join_name(&FnName::from("main"));
        let relative = def_path
            .rvs_strip_prefix(&prefix)
            .expect("def path should have local crate prefix");
        let output = format!(
            "fn={}\nrelative={}\nstarts_with={}\nroot_main={}",
            def_path.rvs_fn_name(),
            relative,
            def_path.rvs_starts_with(&prefix),
            root_main,
        );
        rvs_snapshot_BIS("test_20260702_def_path_strips_local_prefix", &output);
        assert_eq!(def_path.rvs_fn_name().rvs_as_str(), "main");
        assert_eq!(relative.rvs_as_str(), "cli::main");
        assert!(def_path.rvs_starts_with(&prefix));
        assert_eq!(root_main.rvs_as_str(), "cargo_rivus::main");
    }

    #[test]
    fn test_20260703_fn_name_ignores_trait_impl_suffix() {
        let def_path = DefPath::rvs_new("demo::Adapter::rvs_fetch_BI@demo::Client");
        let prefix = CrateName::from("demo").rvs_prefix();
        let relative = def_path
            .rvs_strip_prefix(&prefix)
            .expect("def path should have local prefix");
        let output = format!(
            "def_name={}\nrelative_name={}\n",
            def_path.rvs_fn_name(),
            relative.rvs_fn_name(),
        );
        rvs_snapshot_BIS("test_20260703_fn_name_ignores_trait_impl_suffix", &output);

        assert_eq!(
            rvs_function_name_segment("demo::Adapter::rvs_fetch_BI@demo::Client"),
            "rvs_fetch_BI"
        );
        assert_eq!(def_path.rvs_fn_name().rvs_as_str(), "rvs_fetch_BI");
        assert_eq!(relative.rvs_fn_name().rvs_as_str(), "rvs_fetch_BI");
    }

    #[test]
    fn test_20260712_trait_method_identity_is_structured() {
        let def_path = DefPath::rvs_new("demo::Adapter::rvs_fetch_BI@demo::Client");
        let identity = def_path
            .rvs_trait_method_identity()
            .expect("expected canonical trait method identity");
        let lookup = identity.rvs_lookup_key();
        let trait_method = identity.rvs_trait_method_path();
        let declaration_lookup = TraitMethodKey::rvs_from_trait_method(&trait_method)
            .expect("expected trait declaration lookup key");
        let malformed = [
            DefPath::rvs_new("demo::Adapter::rvs_fetch_BI@"),
            DefPath::rvs_new("@demo::Client"),
            DefPath::rvs_new("demo::Adapter::rvs_fetch_BI@demo::Client@extra"),
        ];
        let output = format!(
            "implementation={}\nmethod={}\ntrait={}\nlookup={}\ntrait_method={}\ndeclaration_lookup={}\nmalformed_rejected={}\n",
            identity.implementation_path,
            identity.rvs_method_name(),
            identity.trait_path,
            lookup.rvs_as_str(),
            trait_method,
            declaration_lookup.rvs_as_str(),
            malformed
                .iter()
                .all(|path| path.rvs_trait_method_identity().is_none()),
        );
        rvs_snapshot_BIS("test_20260712_trait_method_identity_is_structured", &output);

        assert_eq!(identity.implementation_path, "demo::Adapter::rvs_fetch_BI");
        assert_eq!(identity.rvs_method_name(), "rvs_fetch_BI");
        assert_eq!(identity.trait_path, "demo::Client");
        assert_eq!(lookup.rvs_as_str(), "rvs_fetch_BI@demo::Client");
        assert_eq!(trait_method.rvs_as_str(), "demo::Client::rvs_fetch_BI");
        assert_eq!(declaration_lookup, lookup);
        assert!(TraitMethodKey::rvs_from_trait_method(&def_path).is_none());
        assert!(TraitMethodKey::rvs_from_trait_method(&DefPath::rvs_new("plain")).is_none());
        assert!(TraitMethodKey::rvs_from_trait_method(&DefPath::rvs_new("demo::")).is_none());
        assert!(
            malformed
                .iter()
                .all(|path| path.rvs_trait_method_identity().is_none())
        );
    }
}
