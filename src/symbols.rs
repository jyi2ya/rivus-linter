use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::capability::rvs_function_name_segment;

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
}
