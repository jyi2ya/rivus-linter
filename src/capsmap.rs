use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use snafu::Snafu;

use crate::capability::{CapabilityParseError, CapabilitySet};
use crate::symbols::CapsMapKey;

/// 能力之鉴：非 rvs 函数的品行录。
/// 外人虽无 rvs 前缀，登记在册，亦知其能。
#[derive(Debug, Clone, Default)]
pub struct CapsMap {
    entries: BTreeMap<CapsMapKey, CapabilitySet>,
}

#[derive(Debug, Snafu)]
pub enum CapsMapError {
    #[snafu(display("line {line}: invalid capability string '{caps}' for '{key}'"))]
    InvalidCaps {
        key: CapsMapKey,
        caps: String,
        line: usize,
        source: CapabilityParseError,
    },
    #[snafu(display("line {line}: missing '=' separator"))]
    MissingSeparator { line: usize },
    #[snafu(display("line {line}: empty capsmap key"))]
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
}

/// 固定层级顺序。后加载的覆盖先加载的。
/// 这是整个系统中唯一的层级定义——所有调用者都引用这一个常量。
const LAYER_ORDER: &[&str] = &["std", "deps", "seed", "suppress", "ext"];

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

    /// 解析文本为 capsmap。
    ///
    /// 格式：每行 `key=caps` 或 `key=`（表示纯函数）。
    /// 注释以 `#` 开头，但仅从 `=` 之后的值部分剥离——
    /// 键中可含 `#`（如 `closure#0`），因此不从键中剥离注释。
    pub fn rvs_parse(content: &str) -> Result<Self, CapsMapError> {
        let mut entries = BTreeMap::new();
        let mut first_lines = BTreeMap::new();
        for (i, raw_line) in content.lines().enumerate() {
            let line_num = i + 1;
            let trimmed = raw_line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (key, value) = trimmed
                .split_once('=')
                .ok_or(CapsMapError::MissingSeparator { line: line_num })?;
            if key.trim().is_empty() {
                return Err(CapsMapError::EmptyKey { line: line_num });
            }
            let key = CapsMapKey::rvs_new(key.trim().to_string());
            if let Some(first_line) = first_lines.get(&key) {
                return Err(CapsMapError::DuplicateKey {
                    key,
                    first_line: *first_line,
                    line: line_num,
                });
            }
            let value = value.split('#').next().unwrap_or("").trim();
            let caps =
                CapabilitySet::rvs_from_str(value).map_err(|e| CapsMapError::InvalidCaps {
                    key: key.clone(),
                    caps: value.to_string(),
                    line: line_num,
                    source: e,
                })?;
            first_lines.insert(key.clone(), line_num);
            entries.insert(key, caps);
        }
        Ok(Self { entries })
    }

    /// 精确匹配查找，不做后缀匹配。
    pub fn rvs_lookup(&self, name: &str) -> Option<&CapabilitySet> {
        self.entries.get(name)
    }

    /// Insert one typed exact-key entry, replacing any existing value.
    pub(crate) fn rvs_insert_M(&mut self, key: CapsMapKey, caps: CapabilitySet) {
        self.entries.insert(key, caps);
    }

    /// Extend from typed exact-key entries, with later entries taking precedence.
    pub(crate) fn rvs_extend_entries_M(
        &mut self,
        entries: impl IntoIterator<Item = (CapsMapKey, CapabilitySet)>,
    ) {
        for (key, caps) in entries {
            self.rvs_insert_M(key, caps);
        }
    }

    /// 合并另一个 capsmap，后者覆盖前者。
    pub(crate) fn rvs_extend_from_M(&mut self, other: Self) {
        self.rvs_extend_entries_M(other.entries);
    }

    #[cfg(test)]
    pub(crate) fn rvs_to_text(&self) -> String {
        let mut out = String::new();
        for (key, caps) in &self.entries {
            out.push_str(key.rvs_as_str());
            out.push('=');
            out.push_str(&crate::inference::rvs_caps_to_string(caps));
            out.push('\n');
        }
        out
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
        let content = std::fs::read_to_string(&path).map_err(|e| CapsMapError::FileRead {
            path: path.display().to_string(),
            error: e.to_string(),
        })?;
        let partial = rvs_parse_caps_file(&path.display().to_string(), &content)?;
        result.rvs_extend_from_M(partial);
    }
    Ok(result)
}

fn rvs_parse_caps_file(path: &str, content: &str) -> Result<CapsMap, CapsMapError> {
    CapsMap::rvs_parse(content).map_err(|source| CapsMapError::FileParse {
        path: path.to_string(),
        source: Box::new(source),
    })
}

fn rvs_caps_layer_file_path(dir: &Path, layer: &str) -> Result<PathBuf, CapsMapError> {
    let mut components = Path::new(layer).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(dir.join(layer)),
        _ => Err(CapsMapError::InvalidLayerName {
            layer: layer.to_string(),
        }),
    }
}

fn rvs_collect_selected_caps_dir_files_BIS(
    dir: &Path,
    selection: CapsDirSelection<'_>,
) -> Result<Vec<PathBuf>, CapsMapError> {
    match selection {
        CapsDirSelection::All => rvs_collect_caps_dir_files_BIS(dir, &[]),
        CapsDirSelection::Include(layers) => layers
            .iter()
            .map(|layer| rvs_caps_layer_file_path(dir, layer))
            .collect(),
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
        if rvs_is_atomic_caps_temp_file(&name) {
            continue;
        }
        if path.is_file() {
            files.push(path);
        } else if file_type.is_symlink() || LAYER_ORDER.contains(&name.as_str()) {
            return Err(CapsMapError::PathMustBeFile {
                path: path.display().to_string(),
            });
        }
    }
    Ok(files)
}

fn rvs_is_atomic_caps_temp_file(name: &str) -> bool {
    let Some(without_tmp) = name.strip_suffix(".tmp") else {
        return false;
    };
    let Some((without_attempt, attempt)) = without_tmp.rsplit_once('.') else {
        return false;
    };
    let Some((layer, pid)) = without_attempt.rsplit_once('.') else {
        return false;
    };
    layer.starts_with('.')
        && layer.len() > 1
        && !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !attempt.is_empty()
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
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
fn rvs_sort_by_layer_M(files: &mut [std::path::PathBuf]) {
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
    use crate::test_support::{rvs_make_temp_dir_BIS, rvs_snapshot_BIS};

    #[test]
    fn test_20260709_capsmap_parse_and_lookup_table() {
        let parse_cases = [
            ("new_empty", CapsMap::rvs_new(), "anything", None),
            (
                "single",
                CapsMap::rvs_parse("std::fs::read=BI").unwrap(),
                "std::fs::read",
                Some("BI"),
            ),
            (
                "empty_value",
                CapsMap::rvs_parse("HashMap::new=").unwrap(),
                "HashMap::new",
                Some(""),
            ),
            (
                "comments",
                CapsMap::rvs_parse("# comment\nfunc=BI # inline\n").unwrap(),
                "func",
                Some("BI"),
            ),
            (
                "hash_in_key",
                CapsMap::rvs_parse("exr::image::closure#0::crop_samples=S # SideEffect").unwrap(),
                "exr::image::closure#0::crop_samples",
                Some("S"),
            ),
            (
                "empty_content",
                CapsMap::rvs_parse("").unwrap(),
                "anything",
                None,
            ),
            (
                "all_caps",
                CapsMap::rvs_parse("danger=ABIMPSTU").unwrap(),
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

        let lookup = CapsMap::rvs_parse("HashMap::new=").unwrap();
        assert!(lookup.rvs_lookup("HashMap::new").is_some());
        assert!(lookup.rvs_lookup("HashMap").is_none());
        assert!(lookup.rvs_lookup("nonexistent").is_none());
        rvs_snapshot_BIS("test_20260709_capsmap_parse_and_lookup_table", &output);
    }

    #[test]
    fn test_20260709_capsmap_parse_error_table() {
        let cases = [
            ("missing_separator", "no_separator"),
            ("empty_key", "=BI"),
            ("invalid_caps", "func=XYZ"),
            ("duplicate_caps", "func=BB"),
            ("duplicate_key", "func=B\nother=\nfunc=I"),
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
    fn test_20260705_capsmap_to_text_is_deterministic() {
        let cm = CapsMap::rvs_parse("zeta=S\nalpha=BI\n").unwrap();
        let text = cm.rvs_to_text();
        rvs_snapshot_BIS("test_20260705_capsmap_to_text_is_deterministic", &text);

        assert_eq!(text, "alpha=BI\nzeta=S\n");
    }

    #[test]
    fn test_20260611_seed_overrides_std() {
        let dir = std::env::temp_dir().join("test_20260611_seed_overrides_std");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seed"), "func=S\nother_func=T\n").unwrap();
        std::fs::write(dir.join("std"), "func=U\nother_func=U\nnew_func=M\n").unwrap();
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
        std::fs::write(dir.join("seed"), "func_a=S\n").unwrap();
        std::fs::write(dir.join("suppress"), "func_b=\n").unwrap();
        std::fs::write(dir.join("std"), "func_c=M\n").unwrap();
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
        std::fs::write(dir.join("seed"), "func=S\n").unwrap();

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
        std::fs::write(dir.join("seed"), "func=S\n").unwrap();
        std::fs::write(dir.join("suppress"), "func=\n").unwrap();

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
            std::fs::write(dir.join(name), format!("winner={caps}\n")).unwrap();
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
        let result = rvs_parse_caps_file("caps/seed", "broken=E");
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
        std::fs::write(dir.join("seed"), "func_a=S\n").unwrap();
        std::fs::write(dir.join("deps"), "func_b=T\n").unwrap();
        std::fs::write(dir.join("ext"), "func_c=M\n").unwrap();
        let cm = CapsMap::rvs_load_dir_excluding_BIS(&dir, &["deps"]).unwrap();
        assert!(cm.rvs_lookup("func_a").is_some());
        assert!(cm.rvs_lookup("func_b").is_none());
        assert!(cm.rvs_lookup("func_c").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_20260714_caps_loader_ignores_atomic_temp() {
        let dir = rvs_make_temp_dir_BIS("capsmap-ignore-atomic-temp");
        std::fs::write(dir.join("ext"), "winner=P\n").unwrap();
        std::fs::write(dir.join(".deps.123.0.tmp"), "winner=S\n").unwrap();

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
        let mut base = CapsMap::rvs_parse("alpha=B\n").unwrap();
        let extra = CapsMap::rvs_parse("beta=I\n").unwrap();
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
