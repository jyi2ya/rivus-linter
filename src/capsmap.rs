use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::capability::{
    CapabilityBasis, CapabilityCompleteness, CapabilityInfo, CapabilityKnowledgeError,
    CapabilitySet, CapabilitySource,
};
use crate::symbols::{CapsMapKey, DefPath};

pub(crate) const CAPS_V2_HEADER: &str = "# rivus-caps-v2";

/// 能力之鉴：非 rvs 函数的品行录。
/// 外人虽无 rvs 前缀，登记在册，亦知其能。
#[derive(Debug, Clone, Default)]
pub struct CapsMap {
    entries: BTreeMap<CapsMapKey, CapabilityInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapsRecord {
    path: CapsMapKey,
    caps: CapabilitySet,
    basis: CapabilityBasis,
    completeness: CapabilityCompleteness,
}

#[derive(Debug, Snafu)]
pub enum CapsMapError {
    #[snafu(display(
        "capsmap v2 header '{expected}' is required; run `cargo rivus migrate-caps` for legacy v1 files"
    ))]
    MissingV2Header { expected: &'static str },
    #[snafu(display("line {line}: invalid capsmap v2 record: {source}"))]
    InvalidV2Record {
        line: usize,
        source: serde_json::Error,
    },
    #[snafu(display("line {line}: invalid capability knowledge for '{path}': {source}"))]
    InvalidCapabilityKnowledge {
        line: usize,
        path: CapsMapKey,
        source: CapabilityKnowledgeError,
    },
    #[snafu(display("line {line}: empty capsmap path"))]
    EmptyKey { line: usize },
    #[snafu(display(
        "line {line}: duplicate capsmap key '{key}' (first defined on line {first_line})"
    ))]
    DuplicateKey {
        key: CapsMapKey,
        first_line: usize,
        line: usize,
    },
    #[snafu(display("cannot read caps directory: {message}"))]
    DirRead { message: String },
    #[snafu(display("cannot read {path}: {error}"))]
    FileRead { path: String, error: String },
    #[snafu(display("{path}: {source}"))]
    FileParse {
        path: String,
        source: Box<CapsMapError>,
    },
    #[snafu(display("capsmap path must be a directory: {path}"))]
    PathMustBeDirectory { path: String },
    #[snafu(display("capsmap layer must be a file: {path}"))]
    PathMustBeFile { path: String },
    #[snafu(display("capsmap layer name must be a single file name: {layer}"))]
    InvalidLayerName { layer: String },
    #[snafu(display(
        "capsmap layer '{layer}' aliases reserved layer '{expected}'; use the canonical lowercase name"
    ))]
    NonCanonicalLayerName {
        layer: String,
        expected: &'static str,
    },
}

/// 固定层级顺序。后加载的覆盖先加载的。
/// 这是整个系统中唯一的层级定义——所有调用者都引用这一个常量。
const LAYER_ORDER: &[&str] = &["std", "deps", "seed", "suppress", "ext"];

pub(crate) fn rvs_reserved_layer_name(name: &OsStr) -> Option<&'static str> {
    let name = name.to_str()?;
    LAYER_ORDER
        .iter()
        .copied()
        .find(|reserved| name.eq_ignore_ascii_case(reserved))
}

#[derive(Debug, Clone, Copy)]
enum CapsDirSelection<'a> {
    All,
    Include(&'a [&'a str]),
    Exclude(&'a [&'a OsStr]),
}

impl CapsMap {
    /// 构造一个空的能力映射表。
    pub fn rvs_new() -> Self {
        Self::default()
    }

    /// Parse a versioned JSON-lines capsmap.
    pub fn rvs_parse(content: &str) -> Result<Self, CapsMapError> {
        Self::rvs_parse_with_source(content, None)
    }

    fn rvs_parse_with_source(
        content: &str,
        source: Option<(&str, &Path)>,
    ) -> Result<Self, CapsMapError> {
        let mut entries = BTreeMap::new();
        let mut first_lines = BTreeMap::new();
        let mut saw_header = false;
        for (i, raw_line) in content.lines().enumerate() {
            let line_num = i + 1;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !saw_header {
                if trimmed != CAPS_V2_HEADER {
                    return Err(CapsMapError::MissingV2Header {
                        expected: CAPS_V2_HEADER,
                    });
                }
                saw_header = true;
                continue;
            }
            if trimmed.starts_with('#') {
                continue;
            }
            let record: CapsRecord =
                serde_json::from_str(trimmed).map_err(|source| CapsMapError::InvalidV2Record {
                    line: line_num,
                    source,
                })?;
            if record.path.rvs_as_str().is_empty() {
                return Err(CapsMapError::EmptyKey { line: line_num });
            }
            if let Some(first_line) = first_lines.get(&record.path) {
                return Err(CapsMapError::DuplicateKey {
                    key: record.path,
                    first_line: *first_line,
                    line: line_num,
                });
            }
            let key = record.path;
            let mut info = CapabilityInfo::rvs_new(record.caps, record.basis, record.completeness);
            info.rvs_check_invariants().map_err(|source| {
                CapsMapError::InvalidCapabilityKnowledge {
                    line: line_num,
                    path: key.clone(),
                    source,
                }
            })?;
            if let Some((layer, file)) = source {
                info.rvs_with_source_M(CapabilitySource {
                    layer: layer.to_string(),
                    file: file.to_path_buf(),
                    line: line_num,
                });
            }
            first_lines.insert(key.clone(), line_num);
            entries.insert(key, info);
        }
        if !saw_header {
            return Err(CapsMapError::MissingV2Header {
                expected: CAPS_V2_HEADER,
            });
        }
        Ok(Self { entries })
    }

    /// 精确匹配查找，不做后缀匹配。
    #[cfg(test)]
    pub fn rvs_lookup(&self, name: &str) -> Option<&CapabilitySet> {
        self.rvs_lookup_info(name).map(CapabilityInfo::rvs_caps)
    }

    pub(crate) fn rvs_lookup_info(&self, name: &str) -> Option<&CapabilityInfo> {
        self.entries.get(name)
    }

    /// Look up an exact internal path, then its user-facing wildcard path.
    pub(crate) fn rvs_lookup_def_path(&self, path: &DefPath) -> Option<&CapabilitySet> {
        self.rvs_lookup_info_def_path(path)
            .map(CapabilityInfo::rvs_caps)
    }

    pub(crate) fn rvs_lookup_info_def_path(&self, path: &DefPath) -> Option<&CapabilityInfo> {
        self.rvs_lookup_info(path.rvs_as_str()).or_else(|| {
            let user_path = path.rvs_user_path();
            self.rvs_lookup_info(user_path.as_ref())
        })
    }

    /// Insert one typed exact-key entry, replacing any existing value.
    #[cfg(test)]
    pub(crate) fn rvs_insert_M(&mut self, key: CapsMapKey, caps: CapabilitySet) {
        self.rvs_insert_info_M(key, CapabilityInfo::rvs_explicit(caps));
    }

    pub(crate) fn rvs_insert_info_M(&mut self, key: CapsMapKey, info: CapabilityInfo) {
        self.entries.insert(key, info);
    }

    /// Extend from typed exact-key entries, with later entries taking precedence.
    pub(crate) fn rvs_extend_info_entries_M(
        &mut self,
        entries: impl IntoIterator<Item = (CapsMapKey, CapabilityInfo)>,
    ) {
        for (key, info) in entries {
            self.rvs_insert_info_M(key, info);
        }
    }

    /// 合并另一个 capsmap，后者覆盖前者。
    pub(crate) fn rvs_extend_from_M(&mut self, other: Self) {
        self.rvs_extend_info_entries_M(other.entries);
    }

    pub(crate) fn rvs_render_v2(&self) -> String {
        let mut out = String::from(CAPS_V2_HEADER);
        out.push('\n');
        for (path, info) in &self.entries {
            let record = CapsRecord {
                path: path.clone(),
                caps: info.rvs_caps().clone(),
                basis: info.rvs_basis().clone(),
                completeness: info.rvs_completeness(),
            };
            let line = serde_json::to_string(&record)
                .expect("never: capsmap records contain only JSON-compatible data");
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    pub(crate) fn rvs_iter(&self) -> impl Iterator<Item = (&CapsMapKey, &CapabilityInfo)> {
        self.entries.iter()
    }

    /// 加载目录中所有 caps 文件，按固定层级顺序合并。
    ///
    /// 层级顺序：std → deps → seed → suppress → ext → 其余按字母序。
    /// 后加载的覆盖先加载的同名条目。
    pub fn rvs_load_dir_BIS(dir: &Path) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(dir, CapsDirSelection::All)
    }

    /// 加载目录中指定的层级子集。
    /// 例如 `&["seed", "suppress"]` 只加载这两个文件。
    pub fn rvs_load_dir_layers_BIS(dir: &Path, layers: &[&str]) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(dir, CapsDirSelection::Include(layers))
    }

    /// 加载目录中除指定层级外的所有文件。
    /// 例如 `&["deps"]` 加载 std/seed/suppress/ext 但不加载 deps。
    #[cfg(test)]
    pub fn rvs_load_dir_excluding_BIS(dir: &Path, exclude: &[&str]) -> Result<Self, CapsMapError> {
        let exclude = exclude.iter().map(OsStr::new).collect::<Vec<_>>();
        rvs_load_caps_dir_BIS(dir, CapsDirSelection::Exclude(&exclude))
    }

    /// Load all caps files except exact raw layer names.
    pub(crate) fn rvs_load_dir_excluding_names_BIS(
        dir: &Path,
        exclude: &[&OsStr],
    ) -> Result<Self, CapsMapError> {
        rvs_load_caps_dir_BIS(dir, CapsDirSelection::Exclude(exclude))
    }

    /// 统一加载入口：只接受 caps 目录。
    pub fn rvs_load_BIS(path: &Path) -> Result<Self, CapsMapError> {
        if path.is_dir() {
            Self::rvs_load_dir_BIS(path)
        } else {
            Err(CapsMapError::PathMustBeDirectory {
                path: path.display().to_string(),
            })
        }
    }
}

fn rvs_load_caps_dir_BIS(
    dir: &Path,
    selection: CapsDirSelection<'_>,
) -> Result<CapsMap, CapsMapError> {
    if let CapsDirSelection::Include(layers) = selection {
        for layer in layers {
            let _ = rvs_caps_layer_file_path(dir, layer)?;
        }
    }
    let mut result = CapsMap::rvs_new();
    if !rvs_caps_dir_exists_BIS(dir)? {
        return Ok(result);
    }
    rvs_require_caps_dir_BIS(dir)?;
    let mut files = rvs_collect_selected_caps_dir_files_BIS(dir, selection)?;
    rvs_sort_by_layer_M(&mut files);
    for path in files {
        if matches!(selection, CapsDirSelection::Include(_))
            && !rvs_optional_caps_layer_file_BIS(&path)?
        {
            continue;
        }
        let content = crate::fs_guard::rvs_read_regular_file_no_follow_BIS(&path).map_err(|e| {
            CapsMapError::FileRead {
                path: path.display().to_string(),
                error: e.to_string(),
            }
        })?;
        let partial = rvs_parse_caps_file(&path, &content)?;
        result.rvs_extend_from_M(partial);
    }
    Ok(result)
}

fn rvs_parse_caps_file(path: &Path, content: &str) -> Result<CapsMap, CapsMapError> {
    let layer = path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("<unknown>");
    CapsMap::rvs_parse_with_source(content, Some((layer, path))).map_err(|source| {
        CapsMapError::FileParse {
            path: path.display().to_string(),
            source: Box::new(source),
        }
    })
}

fn rvs_caps_layer_file_path(dir: &Path, layer: &str) -> Result<PathBuf, CapsMapError> {
    let mut components = Path::new(layer).components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) || crate::fs_guard::rvs_is_atomic_sibling_temp_name(layer)
    {
        return Err(CapsMapError::InvalidLayerName {
            layer: layer.to_string(),
        });
    }
    if let Some(expected) = rvs_reserved_layer_name(OsStr::new(layer))
        && layer != expected
    {
        return Err(CapsMapError::NonCanonicalLayerName {
            layer: layer.to_string(),
            expected,
        });
    }
    Ok(dir.join(layer))
}

fn rvs_collect_selected_caps_dir_files_BIS(
    dir: &Path,
    selection: CapsDirSelection<'_>,
) -> Result<Vec<PathBuf>, CapsMapError> {
    match selection {
        CapsDirSelection::All => rvs_collect_caps_dir_files_BIS(dir, &[]),
        CapsDirSelection::Include(layers) => {
            let _ = rvs_collect_caps_dir_files_BIS(dir, &[])?;
            layers
                .iter()
                .map(|layer| rvs_caps_layer_file_path(dir, layer))
                .collect()
        }
        CapsDirSelection::Exclude(exclude) => rvs_collect_caps_dir_files_BIS(dir, exclude),
    }
}

fn rvs_collect_caps_dir_files_BIS(
    dir: &Path,
    exclude: &[&OsStr],
) -> Result<Vec<std::path::PathBuf>, CapsMapError> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| CapsMapError::DirRead {
        message: format!("{}: {e}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| CapsMapError::DirRead {
            message: format!("{}: {e}", dir.display()),
        })?;
        let file_type = entry.file_type().map_err(|e| CapsMapError::DirRead {
            message: format!("{}: {e}", entry.path().display()),
        })?;
        let path = entry.path();
        let raw_name = path.file_name().unwrap_or_else(|| OsStr::new(""));
        if exclude.contains(&raw_name) {
            continue;
        }
        let name = raw_name.to_string_lossy().into_owned();
        if let Some(expected) = rvs_reserved_layer_name(raw_name)
            && name != expected
        {
            return Err(CapsMapError::NonCanonicalLayerName {
                layer: name,
                expected,
            });
        }
        if crate::fs_guard::rvs_is_atomic_sibling_temp_name(&name) {
            continue;
        }
        if file_type.is_symlink() {
            return Err(CapsMapError::PathMustBeFile {
                path: path.display().to_string(),
            });
        }
        if file_type.is_file() {
            files.push(path);
        } else if LAYER_ORDER.contains(&name.as_str()) {
            return Err(CapsMapError::PathMustBeFile {
                path: path.display().to_string(),
            });
        }
    }
    Ok(files)
}

fn rvs_require_caps_dir_BIS(dir: &Path) -> Result<(), CapsMapError> {
    if dir.is_dir() {
        Ok(())
    } else {
        Err(CapsMapError::PathMustBeDirectory {
            path: dir.display().to_string(),
        })
    }
}

fn rvs_caps_dir_exists_BIS(dir: &Path) -> Result<bool, CapsMapError> {
    match std::fs::metadata(dir) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(dir) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Ok(_) => Err(CapsMapError::PathMustBeDirectory {
                    path: dir.display().to_string(),
                }),
                Err(symlink_error) => Err(CapsMapError::DirRead {
                    message: format!("{}: {symlink_error}", dir.display()),
                }),
            }
        }
        Err(e) => Err(CapsMapError::DirRead {
            message: format!("{}: {e}", dir.display()),
        }),
    }
}

fn rvs_optional_caps_layer_file_BIS(path: &Path) -> Result<bool, CapsMapError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(true),
        Ok(_) => Err(CapsMapError::PathMustBeFile {
            path: path.display().to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(path) {
                Err(symlink_error) if symlink_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(false)
                }
                Ok(_) => Err(CapsMapError::PathMustBeFile {
                    path: path.display().to_string(),
                }),
                Err(symlink_error) => Err(CapsMapError::FileRead {
                    path: path.display().to_string(),
                    error: symlink_error.to_string(),
                }),
            }
        }
        Err(e) => Err(CapsMapError::FileRead {
            path: path.display().to_string(),
            error: e.to_string(),
        }),
    }
}

/// 按 LAYER_ORDER 对文件路径排序。
/// 在 LAYER_ORDER 中的文件按层级顺序排，不在的按字母序排在后面。
pub(crate) fn rvs_sort_by_layer_M(files: &mut [std::path::PathBuf]) {
    files.sort_by(|a, b| {
        let a_name = a
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(String::new);
        let b_name = b
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(String::new);
        let a_layer = LAYER_ORDER.iter().position(|&n| n == a_name);
        let b_layer = LAYER_ORDER.iter().position(|&n| n == b_name);
        match (a_layer, b_layer) {
            (Some(al), Some(bl)) => al.cmp(&bl),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a_name
                .cmp(&b_name)
                .then_with(|| a.file_name().cmp(&b.file_name())),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::test_support::{
        rvs_caps_v2, rvs_make_capsmap, rvs_make_temp_dir_BIS, rvs_snapshot_BIS,
    };

    #[test]
    fn test_20260716_selected_layer_validation_rejects_missing_escape_and_atomic_temp() {
        let root = rvs_make_temp_dir_BIS("capsmap-selected-layer-validation");
        let missing = root.join("missing");
        let caps = root.join("caps");
        std::fs::create_dir(&caps).unwrap();
        let temporary_layer = ".deps.123.0.tmp";
        std::fs::write(
            caps.join(temporary_layer),
            rvs_caps_v2(&[("temporary", "S")]),
        )
        .unwrap();

        let missing_escape = CapsMap::rvs_load_dir_layers_BIS(&missing, &["../seed"]);
        let selected_temporary = CapsMap::rvs_load_dir_layers_BIS(&caps, &[temporary_layer]);
        let missing_alias = CapsMap::rvs_load_dir_layers_BIS(&missing, &["DEPS"]);
        let output = format!(
            "missing_escape_error={:?}\nselected_temporary_error={:?}\nmissing_alias_error={:?}\n",
            missing_escape.as_ref().err(),
            selected_temporary.as_ref().err(),
            missing_alias.as_ref().err(),
        );
        std::fs::remove_dir_all(root).unwrap();
        rvs_snapshot_BIS(
            "test_20260716_selected_layer_validation_rejects_missing_escape_and_atomic_temp",
            &output,
        );

        assert!(matches!(
            missing_escape,
            Err(CapsMapError::InvalidLayerName { layer }) if layer == "../seed"
        ));
        assert!(matches!(
            selected_temporary,
            Err(CapsMapError::InvalidLayerName { layer }) if layer == temporary_layer
        ));
        assert!(matches!(
            missing_alias,
            Err(CapsMapError::NonCanonicalLayerName { layer, expected })
                if layer == "DEPS" && expected == "deps"
        ));
    }

    #[test]
    fn test_20260709_capsmap_parse_and_lookup_table() {
        let parse_cases = [
            ("new_empty", CapsMap::rvs_new(), "anything", None),
            (
                "single",
                CapsMap::rvs_parse(&rvs_caps_v2(&[("std::fs::read", "BI")])).unwrap(),
                "std::fs::read",
                Some("BI"),
            ),
            (
                "empty_value",
                CapsMap::rvs_parse(&rvs_caps_v2(&[("HashMap::new", "")])).unwrap(),
                "HashMap::new",
                Some(""),
            ),
            (
                "comments",
                CapsMap::rvs_parse(&format!(
                    "{CAPS_V2_HEADER}\n# comment\n{}",
                    rvs_caps_v2(&[("func", "BI")])
                        .lines()
                        .skip(1)
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
                .unwrap(),
                "func",
                Some("BI"),
            ),
            (
                "hash_in_key",
                CapsMap::rvs_parse(&rvs_caps_v2(&[(
                    "exr::image::closure#0::crop_samples",
                    "S",
                )]))
                .unwrap(),
                "exr::image::closure#0::crop_samples",
                Some("S"),
            ),
            (
                "all_caps",
                CapsMap::rvs_parse(&rvs_caps_v2(&[("danger", "ABIMPSTU")])).unwrap(),
                "danger",
                Some("ABIMPSTU"),
            ),
        ];
        let mut output = String::new();
        for (name, capsmap, key, expected) in parse_cases {
            let actual = capsmap
                .rvs_lookup(key)
                .map(crate::inference::rvs_caps_to_string);
            output.push_str(&format!("{name}: {actual:?}\n"));
            assert_eq!(actual.as_deref(), expected, "{name}");
        }

        let lookup = rvs_make_capsmap(&[("HashMap::new", "")]);
        assert!(lookup.rvs_lookup("HashMap::new").is_some());
        assert!(lookup.rvs_lookup("HashMap").is_none());
        assert!(lookup.rvs_lookup("nonexistent").is_none());
        rvs_snapshot_BIS("test_20260709_capsmap_parse_and_lookup_table", &output);
    }

    #[test]
    fn test_20260715_capsmap_lookup_applies_readable_impl_path_to_all_specializations() {
        let capsmap = rvs_make_capsmap(&[
            ("demo::Worker::rvs_run", "BI"),
            (
                "demo::Worker{impl#64656d6f3a3a576f726b65723c7531363e}::rvs_run",
                "S",
            ),
        ]);
        let wildcard = capsmap
            .rvs_lookup_def_path(&DefPath::from(
                "demo::Worker{impl#64656d6f3a3a576f726b65723c75383e}::rvs_run",
            ))
            .map(crate::inference::rvs_caps_to_string);
        let exact = capsmap
            .rvs_lookup_def_path(&DefPath::from(
                "demo::Worker{impl#64656d6f3a3a576f726b65723c7531363e}::rvs_run",
            ))
            .map(crate::inference::rvs_caps_to_string);
        let output = format!("wildcard={wildcard:?}\nexact={exact:?}\n");
        rvs_snapshot_BIS(
            "test_20260715_capsmap_lookup_applies_readable_impl_path_to_all_specializations",
            &output,
        );

        assert_eq!(wildcard.as_deref(), Some("BI"));
        assert_eq!(exact.as_deref(), Some("S"));
    }

    #[test]
    fn test_20260715_capsmap_rejects_case_aliased_reserved_layer() {
        let dir = rvs_make_temp_dir_BIS("capsmap-case-aliased-reserved-layer");
        std::fs::write(dir.join("ext"), rvs_caps_v2(&[("winner", "S")])).unwrap();
        std::fs::write(dir.join("DEPS"), rvs_caps_v2(&[("winner", "B")])).unwrap();

        let all = CapsMap::rvs_load_dir_BIS(&dir);
        let selected = CapsMap::rvs_load_dir_layers_BIS(&dir, &["seed", "suppress"]);
        let output = format!("all={all:?}\nselected={selected:?}\n");
        rvs_snapshot_BIS(
            "test_20260715_capsmap_rejects_case_aliased_reserved_layer",
            &output,
        );

        assert!(all.is_err());
        assert!(selected.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260709_capsmap_parse_error_table() {
        let cases = [
            ("missing_header", "{}"),
            (
                "empty_key",
                "# rivus-caps-v2\n{\"path\":\"\",\"caps\":\"BI\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
            ),
            (
                "invalid_caps",
                "# rivus-caps-v2\n{\"path\":\"func\",\"caps\":\"XYZ\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
            ),
            (
                "duplicate_caps",
                "# rivus-caps-v2\n{\"path\":\"func\",\"caps\":\"BB\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
            ),
            (
                "duplicate_key",
                "# rivus-caps-v2\n{\"path\":\"func\",\"caps\":\"B\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n{\"path\":\"other\",\"caps\":\"\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n{\"path\":\"func\",\"caps\":\"I\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
            ),
        ];
        let mut output = String::new();
        for (name, input) in cases {
            let result = CapsMap::rvs_parse(input);
            output.push_str(&format!("{name}: {result:?}\n"));
            assert!(result.is_err(), "{name}");
        }
        rvs_snapshot_BIS("test_20260709_capsmap_parse_error_table", &output);
    }

    #[test]
    fn test_20260715_capsmap_rejects_inconsistent_knowledge_metadata() {
        let cases = [
            (
                "explicit_incomplete",
                "{\"path\":\"func\",\"caps\":\"B\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"incomplete\"}",
            ),
            (
                "inferred_unknown",
                "{\"path\":\"func\",\"caps\":\"B\",\"basis\":{\"kind\":\"inferred\"},\"completeness\":\"unknown\"}",
            ),
            (
                "port_wrong_caps",
                "{\"path\":\"func\",\"caps\":\"BI\",\"basis\":{\"kind\":\"port\"},\"completeness\":\"complete\"}",
            ),
            (
                "migrated_complete",
                "{\"path\":\"func\",\"caps\":\"B\",\"basis\":{\"kind\":\"migrated_v1\"},\"completeness\":\"complete\"}",
            ),
            (
                "vote_without_implementations",
                "{\"path\":\"func\",\"caps\":\"\",\"basis\":{\"kind\":\"trait_vote\",\"implementations\":0,\"threshold\":0,\"votes\":{}},\"completeness\":\"complete\"}",
            ),
            (
                "vote_wrong_threshold",
                "{\"path\":\"func\",\"caps\":\"\",\"basis\":{\"kind\":\"trait_vote\",\"implementations\":3,\"threshold\":1,\"votes\":{}},\"completeness\":\"complete\"}",
            ),
            (
                "vote_non_propagated_cap",
                "{\"path\":\"func\",\"caps\":\"\",\"basis\":{\"kind\":\"trait_vote\",\"implementations\":3,\"threshold\":2,\"votes\":{\"A\":1}},\"completeness\":\"complete\"}",
            ),
            (
                "vote_count_exceeds_implementations",
                "{\"path\":\"func\",\"caps\":\"B\",\"basis\":{\"kind\":\"trait_vote\",\"implementations\":3,\"threshold\":2,\"votes\":{\"B\":4}},\"completeness\":\"complete\"}",
            ),
            (
                "vote_selected_caps_mismatch",
                "{\"path\":\"func\",\"caps\":\"\",\"basis\":{\"kind\":\"trait_vote\",\"implementations\":3,\"threshold\":2,\"votes\":{\"S\":2}},\"completeness\":\"complete\"}",
            ),
        ];
        let mut output = String::new();
        for (name, record) in cases {
            let input = format!("{CAPS_V2_HEADER}\n{record}\n");
            let error = CapsMap::rvs_parse(&input)
                .expect_err("invalid knowledge metadata must be rejected");
            output.push_str(&format!("{name}: {error}\n"));
        }
        rvs_snapshot_BIS(
            "test_20260715_capsmap_rejects_inconsistent_knowledge_metadata",
            &output,
        );
    }

    #[test]
    fn test_20260705_capsmap_to_text_is_deterministic() {
        let cm = rvs_make_capsmap(&[("zeta", "S"), ("alpha", "BI")]);
        let text = cm.rvs_render_v2();
        rvs_snapshot_BIS("test_20260705_capsmap_to_text_is_deterministic", &text);

        assert!(text.starts_with(CAPS_V2_HEADER));
        assert!(text.find("alpha").unwrap() < text.find("zeta").unwrap());
    }

    #[test]
    fn test_20260715_capsmap_v2_roundtrip_preserves_vote_metadata_and_source() {
        let dir = rvs_make_temp_dir_BIS("capsmap-v2-vote-roundtrip");
        let mut map = CapsMap::rvs_new();
        map.rvs_insert_info_M(
            CapsMapKey::from("demo::FromString::rvs_parse"),
            CapabilityInfo::rvs_trait_vote(
                CapabilitySet::rvs_new(),
                3,
                2,
                BTreeMap::from([(Capability::S, 1)]),
                CapabilityCompleteness::Complete,
            ),
        );
        std::fs::write(dir.join("std"), map.rvs_render_v2()).unwrap();

        let loaded = CapsMap::rvs_load_dir_BIS(&dir).unwrap();
        let info = loaded
            .rvs_lookup_info("demo::FromString::rvs_parse")
            .unwrap();
        let source = info.rvs_source().unwrap();
        let output = format!(
            "basis={:?}\ncompleteness={}\nsource_layer={}\nsource_file={}\nsource_line={}\n",
            info.rvs_basis(),
            info.rvs_completeness().rvs_name(),
            source.layer,
            source.file.file_name().unwrap().to_string_lossy(),
            source.line,
        );
        rvs_snapshot_BIS(
            "test_20260715_capsmap_v2_roundtrip_preserves_vote_metadata_and_source",
            &output,
        );

        assert!(matches!(
            info.rvs_basis(),
            CapabilityBasis::TraitVote {
                implementations: 3,
                threshold: 2,
                ..
            }
        ));
        assert_eq!(source.line, 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260611_seed_overrides_std() {
        let dir = std::env::temp_dir().join("test_20260611_seed_overrides_std");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("seed"),
            rvs_caps_v2(&[("func", "S"), ("other_func", "T")]),
        )
        .unwrap();
        std::fs::write(
            dir.join("std"),
            rvs_caps_v2(&[("func", "U"), ("other_func", "U"), ("new_func", "M")]),
        )
        .unwrap();
        let cm = CapsMap::rvs_load_dir_BIS(&dir).unwrap();
        let caps = cm.rvs_lookup("func").unwrap();
        assert!(caps.rvs_contains(Capability::S));
        assert!(!caps.rvs_contains(Capability::U));
        let other = cm.rvs_lookup("other_func").unwrap();
        assert!(other.rvs_contains(Capability::T));
        assert!(!other.rvs_contains(Capability::U));
        let new_func = cm.rvs_lookup("new_func").unwrap();
        assert!(new_func.rvs_contains(Capability::M));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260615_load_dir_layers() {
        let dir = std::env::temp_dir().join("test_20260615_load_dir_layers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), rvs_caps_v2(&[("func_a", "S")])).unwrap();
        std::fs::write(dir.join("suppress"), rvs_caps_v2(&[("func_b", "")])).unwrap();
        std::fs::write(dir.join("std"), rvs_caps_v2(&[("func_c", "M")])).unwrap();
        let cm = CapsMap::rvs_load_dir_layers_BIS(&dir, &["seed", "suppress"]).unwrap();
        assert!(cm.rvs_lookup("func_a").is_some());
        assert!(cm.rvs_lookup("func_b").is_some());
        assert!(cm.rvs_lookup("func_c").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260708_load_dir_layers_rejects_path_escape() {
        let dir = std::env::temp_dir().join("test_20260708_load_dir_layers_rejects_path_escape");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), rvs_caps_v2(&[("func", "S")])).unwrap();

        let absolute = dir.join("seed").to_string_lossy().into_owned();
        assert!(rvs_caps_layer_file_path(&dir, "seed").is_ok());
        assert!(matches!(
            rvs_caps_layer_file_path(&dir, "../seed"),
            Err(CapsMapError::InvalidLayerName { .. })
        ));
        let results = ["../seed", "nested/seed", ".", absolute.as_str()]
            .map(|layer| CapsMap::rvs_load_dir_layers_BIS(&dir, &[layer]));
        let summary = results
            .iter()
            .map(|result| match result {
                Err(CapsMapError::InvalidLayerName { layer }) => format!("invalid:{layer}"),
                other => format!("unexpected:{other:?}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        rvs_snapshot_BIS(
            "test_20260708_load_dir_layers_rejects_path_escape",
            &format!("{summary}\n").replace(&absolute, "$ABS"),
        );

        assert!(
            results
                .iter()
                .all(|result| matches!(result, Err(CapsMapError::InvalidLayerName { .. })))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260706_load_dir_layers_uses_global_precedence() {
        let dir = std::env::temp_dir().join("test_20260706_load_dir_layers_uses_global_precedence");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), rvs_caps_v2(&[("func", "S")])).unwrap();
        std::fs::write(dir.join("suppress"), rvs_caps_v2(&[("func", "")])).unwrap();

        let cm = CapsMap::rvs_load_dir_layers_BIS(&dir, &["suppress", "seed"]).unwrap();
        let caps = cm.rvs_lookup("func").unwrap();
        rvs_snapshot_BIS(
            "test_20260706_load_dir_layers_uses_global_precedence",
            &format!("caps={}\n", crate::inference::rvs_caps_to_string(caps)),
        );

        assert!(caps.rvs_is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260710_capsmap_selection_preserves_global_precedence_table() {
        let dir = rvs_make_temp_dir_BIS("capsmap-selection-precedence");
        for (name, caps) in [
            ("std", "A"),
            ("deps", "B"),
            ("seed", "I"),
            ("suppress", "M"),
            ("ext", "P"),
            ("alpha", "S"),
            ("zeta", "U"),
        ] {
            std::fs::write(dir.join(name), rvs_caps_v2(&[("winner", caps)])).unwrap();
        }

        let cases = [
            ("all", CapsMap::rvs_load_dir_BIS(&dir).unwrap(), "U"),
            (
                "include_shuffled",
                CapsMap::rvs_load_dir_layers_BIS(&dir, &["zeta", "std", "ext"]).unwrap(),
                "U",
            ),
            (
                "include_layers_shuffled",
                CapsMap::rvs_load_dir_layers_BIS(&dir, &["suppress", "seed"]).unwrap(),
                "M",
            ),
            (
                "exclude_zeta",
                CapsMap::rvs_load_dir_excluding_BIS(&dir, &["zeta"]).unwrap(),
                "S",
            ),
            (
                "exclude_additional",
                CapsMap::rvs_load_dir_excluding_BIS(&dir, &["alpha", "zeta"]).unwrap(),
                "P",
            ),
        ];
        let mut output = String::new();
        for (name, capsmap, expected) in cases {
            let actual =
                crate::inference::rvs_caps_to_string(capsmap.rvs_lookup("winner").unwrap());
            output.push_str(&format!("{name}={actual}\n"));
            assert_eq!(actual, expected, "{name}");
        }
        rvs_snapshot_BIS(
            "test_20260710_capsmap_selection_preserves_global_precedence_table",
            &output,
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_20260706_load_dir_rejects_caps_file_path() {
        let path = std::env::temp_dir().join("test_20260706_load_dir_rejects_caps_file_path");
        std::fs::write(&path, "func=S\n").unwrap();
        let results = [
            CapsMap::rvs_load_dir_BIS(&path).is_err(),
            CapsMap::rvs_load_dir_layers_BIS(&path, &["seed"]).is_err(),
            CapsMap::rvs_load_dir_excluding_BIS(&path, &["deps"]).is_err(),
        ];
        rvs_snapshot_BIS(
            "test_20260706_load_dir_rejects_caps_file_path",
            &format!("results={results:?}\n"),
        );

        assert_eq!(results, [true, true, true]);
        std::fs::remove_file(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260706_load_dir_rejects_broken_caps_symlink() {
        let path = std::env::temp_dir().join("test_20260706_load_dir_rejects_broken_caps_symlink");
        let _ = std::fs::remove_file(&path);
        std::os::unix::fs::symlink(path.with_extension("missing"), &path).unwrap();
        let results = [
            CapsMap::rvs_load_dir_BIS(&path).is_err(),
            CapsMap::rvs_load_dir_layers_BIS(&path, &["seed"]).is_err(),
            CapsMap::rvs_load_dir_excluding_BIS(&path, &["deps"]).is_err(),
        ];
        rvs_snapshot_BIS(
            "test_20260706_load_dir_rejects_broken_caps_symlink",
            &format!("results={results:?}\n"),
        );

        assert_eq!(results, [true, true, true]);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_20260706_load_dir_layers_rejects_layer_directory() {
        let dir =
            std::env::temp_dir().join("test_20260706_load_dir_layers_rejects_layer_directory");
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("seed")).unwrap();

        let layered = CapsMap::rvs_load_dir_layers_BIS(&dir, &["seed"]);
        let full = CapsMap::rvs_load_dir_BIS(&dir);
        rvs_snapshot_BIS(
            "test_20260706_load_dir_layers_rejects_layer_directory",
            &format!("layered={layered:?}\nfull={full:?}\n")
                .replace(&dir.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(layered.is_err());
        assert!(full.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260706_require_caps_dir_rejects_file_path() {
        let path = std::env::temp_dir().join("test_20260706_require_caps_dir_rejects_file_path");
        std::fs::write(&path, "func=S\n").unwrap();
        let result = rvs_require_caps_dir_BIS(&path);
        rvs_snapshot_BIS(
            "test_20260706_require_caps_dir_rejects_file_path",
            &format!("{result:?}\n").replace(&path.to_string_lossy().into_owned(), "$TMP"),
        );

        assert!(result.is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_20260705_capsmap_file_parse_error_includes_path() {
        let result = rvs_parse_caps_file(
            Path::new("caps/seed"),
            "# rivus-caps-v2\n{\"path\":\"broken\",\"caps\":\"E\",\"basis\":{\"kind\":\"explicit\"},\"completeness\":\"complete\"}\n",
        );
        let message = result.unwrap_err().to_string();
        rvs_snapshot_BIS(
            "test_20260705_capsmap_file_parse_error_includes_path",
            &format!("{message}\n"),
        );
        assert!(message.contains("caps/seed"));
        assert!(message.contains("line 1"));
    }

    #[test]
    fn test_20260615_load_dir_excluding() {
        let dir = std::env::temp_dir().join("test_20260615_load_dir_excluding");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), rvs_caps_v2(&[("func_a", "S")])).unwrap();
        std::fs::write(dir.join("deps"), rvs_caps_v2(&[("func_b", "T")])).unwrap();
        std::fs::write(dir.join("ext"), rvs_caps_v2(&[("func_c", "M")])).unwrap();
        let cm = CapsMap::rvs_load_dir_excluding_BIS(&dir, &["deps"]).unwrap();
        assert!(cm.rvs_lookup("func_a").is_some());
        assert!(cm.rvs_lookup("func_b").is_none());
        assert!(cm.rvs_lookup("func_c").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260714_caps_loader_ignores_atomic_temp() {
        let dir = rvs_make_temp_dir_BIS("capsmap-ignore-atomic-temp");
        std::fs::write(dir.join("ext"), rvs_caps_v2(&[("winner", "P")])).unwrap();
        std::fs::write(dir.join(".deps.123.0.tmp"), rvs_caps_v2(&[("winner", "S")])).unwrap();

        let caps = CapsMap::rvs_load_dir_BIS(&dir).unwrap();
        let winner = crate::inference::rvs_caps_to_string(caps.rvs_lookup("winner").unwrap());
        rvs_snapshot_BIS(
            "test_20260714_caps_loader_ignores_atomic_temp",
            &format!("winner={winner}\n"),
        );

        assert_eq!(winner, "P");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_caps_loader_rejects_broken_custom_layer_symlink() {
        let dir = rvs_make_temp_dir_BIS("capsmap-broken-custom-layer");
        let layer = dir.join("custom");
        std::os::unix::fs::symlink(dir.join("missing"), &layer).unwrap();

        let result = CapsMap::rvs_load_dir_BIS(&dir);
        let output = format!("is_err={}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260714_caps_loader_rejects_broken_custom_layer_symlink",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260715_caps_loader_rejects_valid_custom_layer_symlink() {
        let dir = rvs_make_temp_dir_BIS("capsmap-valid-custom-layer-symlink");
        let source = dir.join("source");
        std::fs::write(&source, rvs_caps_v2(&[("value", "S")])).unwrap();
        std::os::unix::fs::symlink(&source, dir.join("custom")).unwrap();

        let result = CapsMap::rvs_load_dir_BIS(&dir);
        let output = format!("is_err={}\n", result.is_err());
        rvs_snapshot_BIS(
            "test_20260715_caps_loader_rejects_valid_custom_layer_symlink",
            &output,
        );

        assert!(result.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_20260714_caps_layer_sort_uses_raw_name_tiebreaker() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let mut files = vec![
            PathBuf::from(std::ffi::OsString::from_vec(vec![0x81])),
            PathBuf::from(std::ffi::OsString::from_vec(vec![0x80])),
        ];
        rvs_sort_by_layer_M(&mut files);
        let order = files
            .iter()
            .map(|path| {
                *path
                    .as_os_str()
                    .as_bytes()
                    .first()
                    .expect("never: test path has one byte")
            })
            .collect::<Vec<_>>();
        let output = format!("order={order:?}\n");
        rvs_snapshot_BIS(
            "test_20260714_caps_layer_sort_uses_raw_name_tiebreaker",
            &output,
        );

        assert_eq!(order, [0x80, 0x81]);
    }

    #[test]
    fn test_20260615_load_single_file() {
        let path = std::env::temp_dir().join("test_20260615_load_single_file.txt");
        std::fs::write(&path, "func=BI\n").unwrap();
        let result = CapsMap::rvs_load_BIS(&path);
        assert!(result.is_err());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_20260615_load_nonexistent() {
        let result = CapsMap::rvs_load_BIS(std::path::Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_20260630_extend_from_and_sort_by_layer() {
        let mut base = rvs_make_capsmap(&[("alpha", "B")]);
        let extra = rvs_make_capsmap(&[("beta", "I")]);
        base.rvs_extend_from_M(extra);
        assert!(
            base.rvs_lookup("alpha")
                .unwrap()
                .rvs_contains(Capability::B)
        );
        assert!(base.rvs_lookup("beta").unwrap().rvs_contains(Capability::I));

        let mut files = vec![
            std::path::PathBuf::from("ext"),
            std::path::PathBuf::from("seed"),
            std::path::PathBuf::from("std"),
        ];
        rvs_sort_by_layer_M(&mut files);
        let ordered: Vec<String> = files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(ordered, vec!["std", "seed", "ext"]);
    }
}
