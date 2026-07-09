use std::path::PathBuf;

pub(crate) fn rvs_snapshot_BIS(name: &str, content: &str) {
    std::fs::create_dir_all("test_out").unwrap();
    std::fs::write(format!("test_out/{name}.out"), content).unwrap();
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
