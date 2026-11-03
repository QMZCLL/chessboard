//! 写文件：先写到同一目录的临时文件并刷到磁盘，再替换目标文件。写到一半时出错或断电，原来的文件保持不变。

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    path.with_file_name(format!("{name}.tmp"))
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = temp_path(path);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_file_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("档案.json");
        std::fs::write(&path, "旧的").unwrap();
        write_atomic(&path, "新的".as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "新的");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "没有留下临时文件");
    }

    #[test]
    fn a_failed_write_keeps_the_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "原来的").unwrap();
        // 临时文件的位置被一个目录占住：写不进去
        std::fs::create_dir(temp_path(&path)).unwrap();
        assert!(write_atomic(&path, b"new").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "原来的");
    }
}
