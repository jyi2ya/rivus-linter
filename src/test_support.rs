use std::ffi::OsStr;
use std::path::PathBuf;

fn rvs_bless_value_enabled(value: Option<&OsStr>) -> bool {
    value.and_then(OsStr::to_str) == Some("1")
}

fn rvs_snapshot_mismatch(name: &str, expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }

    let mut character = 0;
    let mut line = 1;
    let mut column = 1;
    let mut expected_chars = expected.chars();
    let mut actual_chars = actual.chars();
    loop {
        match (expected_chars.next(), actual_chars.next()) {
            (Some(expected_char), Some(actual_char)) if expected_char == actual_char => {
                character += 1;
                if expected_char == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }
            (None, None) => return None,
            _ => break,
        }
    }

    Some(format!(
        "snapshot mismatch for `test_out/{name}.out`\n\
         first difference at character {character} (line {line}, column {column})\n\
         --- expected ---\n{expected}\n\
         --- actual ---\n{actual}\n\
         --- expected (escaped) ---\n{expected:?}\n\
         --- actual (escaped) ---\n{actual:?}\n\
         update intentionally with: RUSTC_BLESS=1 cargo test {name}"
    ))
}

pub(crate) fn rvs_snapshot_BIS(name: &str, content: &str) {
    let relative_path = format!("test_out/{name}.out");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&relative_path);
    if rvs_bless_value_enabled(std::env::var_os("RUSTC_BLESS").as_deref()) {
        std::fs::create_dir_all(path.parent().expect("never: snapshot path has a parent"))
            .unwrap_or_else(|error| panic!("cannot create snapshot directory: {error}"));
        std::fs::write(&path, content)
            .unwrap_or_else(|error| panic!("cannot write snapshot `{relative_path}`: {error}"));
        return;
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(expected) => expected,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => panic!(
            "missing snapshot `{relative_path}`\n\
             create it intentionally with: RUSTC_BLESS=1 cargo test {name}"
        ),
        Err(error) => panic!("cannot read snapshot `{relative_path}`: {error}"),
    };
    if let Some(message) = rvs_snapshot_mismatch(name, &expected, content) {
        panic!("{message}");
    }
}

pub(crate) fn rvs_make_temp_dir_BIS(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("never: system clock should be after unix epoch for test temp dir")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("rivus-{tag}-{}-{unique}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_20260710_snapshot_bless_flag_and_comparison_logic() {
        let cases = [
            (None, false),
            (Some(OsStr::new("")), false),
            (Some(OsStr::new("0")), false),
            (Some(OsStr::new("true")), false),
            (Some(OsStr::new("01")), false),
            (Some(OsStr::new("1")), true),
        ];
        let mut output = String::new();
        for (value, expected) in cases {
            let actual = rvs_bless_value_enabled(value);
            output.push_str(&format!("{value:?}={actual}\n"));
            assert_eq!(actual, expected);
        }

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;

            let non_unicode = std::ffi::OsString::from_vec(vec![0xff]);
            assert!(!rvs_bless_value_enabled(Some(non_unicode.as_os_str())));
        }

        let equal = rvs_snapshot_mismatch("example", "same\n", "same\n");
        assert!(equal.is_none());
        output.push_str("equal=true\n");

        let mismatch = rvs_snapshot_mismatch("example", "alpha\nbeta\n", "alpha\nzeta\n")
            .expect("different snapshot contents should produce a diagnostic");
        output.push_str(&mismatch);
        output.push('\n');
        rvs_snapshot_BIS(
            "test_20260710_snapshot_bless_flag_and_comparison_logic",
            &output,
        );
    }
}
