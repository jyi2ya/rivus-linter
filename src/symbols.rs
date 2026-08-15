use std::borrow::{Borrow, Cow};
use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! rvs_string_symbol {
    ($name:ident, $new_doc:literal, $as_str_doc:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[doc = $new_doc]
            pub fn rvs_new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            #[doc = $as_str_doc]
            pub fn rvs_as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.rvs_as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.rvs_as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.rvs_as_str()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::rvs_new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::rvs_new(value)
            }
        }
    };
}

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

rvs_string_symbol!(
    FnName,
    "Wrap a bare function name.",
    "Borrow the function name as `&str`."
);

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
        let method_name = rvs_function_segment_without_identity_marker(method_name);
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
            rvs_function_segment_without_identity_marker(
                method_path.rsplit("::").next().unwrap_or(method_path),
            )
        },
        TraitMethodIdentity::rvs_method_name,
    )
}

pub(crate) fn rvs_attach_generated_definition_marker_M(parts: &mut [String], marker: &str) {
    debug_assert!(!parts.is_empty(), "def path has a crate or item segment");
    debug_assert!(
        !marker.is_empty(),
        "generated definition marker is nonempty"
    );
    if let Some(generated_item) = parts.last_mut() {
        generated_item.push_str("{def#");
        generated_item.push_str(marker);
        generated_item.push('}');
    }
}

fn rvs_function_segment_without_identity_marker(segment: &str) -> &str {
    rvs_next_identity_marker(segment, 0).map_or(segment, |(start, _)| {
        segment
            .get(..start)
            .expect("never: marker starts on a UTF-8 boundary")
    })
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

    /// Return the user-facing path without internal identity markers.
    pub(crate) fn rvs_user_path(&self) -> Cow<'_, str> {
        rvs_strip_identity_markers(&self.0)
    }

    /// Return the last function-name segment of the def-path.
    pub fn rvs_fn_name(&self) -> FnName {
        FnName::rvs_new(self.rvs_fn_name_str())
    }

    /// Borrow the last function-name segment of the def-path.
    pub fn rvs_fn_name_str(&self) -> &str {
        rvs_function_name_segment(&self.0)
    }

    /// Parse this path as a serialized trait-implementation method identity.
    pub(crate) fn rvs_trait_method_identity(&self) -> Option<TraitMethodIdentity<'_>> {
        TraitMethodIdentity::rvs_parse(&self.0)
    }

    /// Return whether the def-path contains a substring.
    pub fn rvs_contains(&self, needle: &str) -> bool {
        self.rvs_user_path().contains(needle)
    }

    /// Return whether this function lives inside a `tests` module segment.
    ///
    /// A test module is any module whose name is exactly `tests`. Functions
    /// inside `mod tests { ... }` (including `#[test]` functions and test
    /// helpers) are detected by checking whether any path segment equals
    /// `tests` before the final function-name segment.
    pub fn rvs_is_in_test_module(&self) -> bool {
        let path = self.rvs_user_path();
        let mut parts = path.rsplit("::");
        let _ = parts.next();
        parts.any(|segment| segment == "tests")
    }

    /// Return whether this def-path belongs to a `build_script_build` crate
    /// compilation unit.
    ///
    /// Cargo compiles every package's `build.rs` under the same readable crate
    /// name `build_script_build`, so def-paths from different packages collide
    /// in the flat graph. However, a normal package may also be *named*
    /// `build-script-build` (normalized to the same readable crate name), and
    /// its sources must not be mistaken for compile-time machinery. The
    /// caller must therefore combine this name check with the Cargo package
    /// name: a compilation unit is a build script only when its crate name is
    /// `build_script_build` and its Cargo package name normalizes to something
    /// else.
    pub fn rvs_is_build_script_crate(&self) -> bool {
        self.rvs_user_path()
            .split("::")
            .next()
            .is_some_and(|segment| segment == BUILD_SCRIPT_CRATE_NAME)
    }

    /// Return whether this def-path's crate is a build script for the given
    /// Cargo package name.
    ///
    /// See [`DefPath::rvs_is_build_script_crate`] for why the package name is
    /// required: a package legitimately named `build-script-build` compiles to
    /// the same readable crate name as real build scripts, so only the
    /// package-name mismatch distinguishes them.
    pub fn rvs_is_build_script_for_package(&self, cargo_package_name: Option<&str>) -> bool {
        self.rvs_is_build_script_crate()
            && cargo_package_name.is_some_and(|name| {
                CrateName::rvs_from_manifest_name(name).rvs_as_str() != BUILD_SCRIPT_CRATE_NAME
            })
    }
}

/// The readable crate name Cargo assigns to every package's build script.
pub(crate) const BUILD_SCRIPT_CRATE_NAME: &str = "build_script_build";

/// Read the Cargo package name of the current compilation unit.
///
/// Cargo sets `CARGO_PKG_NAME` for every rustc invocation it drives, including
/// build-script compilations (where the value is the owning package's name,
/// e.g. `demo`, not `build_script_build`). A missing variable means rustc was
/// invoked outside Cargo, in which case no build-script conclusion can be
/// drawn from the environment.
pub(crate) fn rvs_cargo_package_name_BS() -> Option<String> {
    match std::env::var("CARGO_PKG_NAME") {
        Ok(name) => Some(name),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(value)) => value.to_string_lossy().into_owned().into(),
    }
}

pub(crate) fn rvs_strip_identity_markers(path: &str) -> Cow<'_, str> {
    let mut cursor = 0usize;
    let mut output = String::new();
    while let Some((start, end)) = rvs_next_identity_marker(path, cursor) {
        output.push_str(
            path.get(cursor..start)
                .expect("never: marker starts on a UTF-8 boundary"),
        );
        cursor = end;
    }
    if cursor == 0 {
        Cow::Borrowed(path)
    } else {
        output.push_str(
            path.get(cursor..)
                .expect("never: marker ends on a UTF-8 boundary"),
        );
        Cow::Owned(output)
    }
}

fn rvs_next_identity_marker(path: &str, start_at: usize) -> Option<(usize, usize)> {
    debug_assert!(start_at <= path.len(), "marker search starts inside path");
    const PREFIXES: [&str; 2] = ["{impl#", "{def#"];
    let mut cursor = start_at;
    while let Some((relative_start, prefix)) = path.get(cursor..).and_then(|rest| {
        PREFIXES
            .into_iter()
            .filter_map(|prefix| rest.find(prefix).map(|start| (start, prefix)))
            .min_by_key(|(start, _)| *start)
    }) {
        let start = cursor + relative_start;
        let digits_start = start + prefix.len();
        let Some(relative_end) = path.get(digits_start..).and_then(|rest| rest.find('}')) else {
            break;
        };
        let end = digits_start + relative_end;
        let digits = path.get(digits_start..end).unwrap_or("");
        if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some((start, end + 1));
        } else {
            cursor = digits_start;
        }
    }
    None
}

impl fmt::Display for DefPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.rvs_user_path().as_ref())
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

rvs_string_symbol!(
    CapsMapKey,
    "Wrap a capsmap key string.",
    "Borrow the capsmap key as `&str`."
);

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
    fn test_20260712_def_path_extracts_nested_main_name() {
        let def_path = DefPath::rvs_new("cargo_rivus::cli::main");
        let output = format!("fn={}\n", def_path.rvs_fn_name());
        rvs_snapshot_BIS("test_20260712_def_path_extracts_nested_main_name", &output);
        assert_eq!(def_path.rvs_fn_name().rvs_as_str(), "main");
    }

    #[test]
    fn test_20260703_fn_name_ignores_trait_impl_suffix() {
        let def_path = DefPath::rvs_new("demo::Adapter::rvs_fetch_BI@demo::Client");
        let output = format!("def_name={}\n", def_path.rvs_fn_name());
        rvs_snapshot_BIS("test_20260703_fn_name_ignores_trait_impl_suffix", &output);

        assert_eq!(
            rvs_function_name_segment("demo::Adapter::rvs_fetch_BI@demo::Client"),
            "rvs_fetch_BI"
        );
        assert_eq!(def_path.rvs_fn_name().rvs_as_str(), "rvs_fetch_BI");
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

    #[test]
    fn test_20260715_def_path_hides_impl_marker_from_users() {
        let path = DefPath::rvs_new(
            "demo::Worker{impl#64656d6f3a3a576f726b65723c75383e}::rvs_run_BI@demo::Runner",
        );
        let output = format!(
            "raw={}\nuser={}\ncontains_user_path={}\n",
            path.rvs_as_str(),
            path,
            path.rvs_contains("Worker::rvs_run_BI")
        );
        rvs_snapshot_BIS(
            "test_20260715_def_path_hides_impl_marker_from_users",
            &output,
        );

        assert_eq!(
            path.rvs_user_path(),
            "demo::Worker::rvs_run_BI@demo::Runner"
        );
        assert_eq!(path.rvs_fn_name_str(), "rvs_run_BI");
        assert!(path.rvs_contains("Worker::rvs_run_BI"));
    }

    #[test]
    fn test_20260731_def_path_hides_generated_definition_marker() {
        let path =
            DefPath::rvs_new("demo::rvs_parent::rvs_generated_S{def#4465665061746848617368}");
        let output = format!(
            "raw={}\nuser={}\nfunction={}\ncontains_user_path={}\n",
            path.rvs_as_str(),
            path,
            path.rvs_fn_name_str(),
            path.rvs_contains("rvs_parent::rvs_generated_S"),
        );
        rvs_snapshot_BIS(
            "test_20260731_def_path_hides_generated_definition_marker",
            &output,
        );

        assert_eq!(path.rvs_user_path(), "demo::rvs_parent::rvs_generated_S");
        assert_eq!(path.rvs_fn_name_str(), "rvs_generated_S");
        assert!(path.rvs_contains("rvs_parent::rvs_generated_S"));
    }

    #[test]
    fn test_20260814_build_script_detection_requires_package_mismatch() {
        let build_script_path = DefPath::rvs_new("build_script_build::main");
        let ordinary_package_path = DefPath::rvs_new("demo::rvs_run");
        let cases = [
            (
                "real build script of package demo",
                build_script_path.clone(),
                Some("demo"),
                true,
            ),
            (
                "ordinary crate named build-script-build",
                build_script_path.clone(),
                Some("build-script-build"),
                false,
            ),
            (
                "package name uses the normalized form",
                build_script_path.clone(),
                Some("build_script_build"),
                false,
            ),
            (
                "rustc invoked outside Cargo",
                build_script_path.clone(),
                None,
                false,
            ),
            (
                "ordinary crate path with foreign package",
                ordinary_package_path.clone(),
                Some("demo"),
                false,
            ),
        ];
        let mut output = String::new();
        for (label, path, package_name, expected) in &cases {
            let actual = path.rvs_is_build_script_for_package(package_name.as_deref());
            output.push_str(&format!("{label}={actual}\n"));
            assert_eq!(actual, *expected, "{label}");
        }
        rvs_snapshot_BIS(
            "test_20260814_build_script_detection_requires_package_mismatch",
            &output,
        );
    }
}
