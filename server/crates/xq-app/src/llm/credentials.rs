//! 模型服务的密钥。
//!
//! 密钥只在后台使用：保存在配置目录下单独的 `credentials.json`（Unix 上权限为 0600），
//! 普通配置只记录引用（`store:coach` 或 `env:变量名`），界面只能看到“是否已配置”。

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

pub const STORE_REF: &str = "store:coach";
const FILE: &str = "credentials.json";
const ENTRY: &str = "coach";

fn path(config_dir: &Path) -> PathBuf { config_dir.join(FILE) }

fn read_all(config_dir: &Path) -> Result<BTreeMap<String, String>, String> {
    match fs::read_to_string(path(config_dir)) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("凭据文件无法解析：{e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("无法读取凭据文件：{e}")),
    }
}

/// 按引用取出密钥；没有引用时返回 None。
pub fn resolve(config_dir: &Path, reference: Option<&str>) -> Result<Option<String>, String> {
    let Some(reference) = reference else { return Ok(None) };
    if reference == STORE_REF {
        return Ok(read_all(config_dir)?.remove(ENTRY).filter(|k| !k.is_empty()));
    }
    if let Some(name) = reference.strip_prefix("env:") {
        return Ok(std::env::var(name).ok().filter(|k| !k.trim().is_empty()));
    }
    Err("凭据引用无效".into())
}

/// 保存或清除密钥（先写临时文件再替换）。
pub fn store(config_dir: &Path, key: Option<&str>) -> Result<(), String> {
    let mut all = read_all(config_dir).unwrap_or_default();
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) => {
            all.insert(ENTRY.to_string(), k.to_string());
        }
        None => {
            all.remove(ENTRY);
        }
    }
    fs::create_dir_all(config_dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    let text = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(|e| format!("无法写入凭据文件：{e}"))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("无法写入凭据文件：{e}"))?;
    drop(file);
    fs::rename(&tmp, path(config_dir)).map_err(|e| format!("无法写入凭据文件：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_resolve() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve(dir.path(), Some(STORE_REF)).unwrap(), None);
        store(dir.path(), Some(" sk-test ")).unwrap();
        assert_eq!(resolve(dir.path(), Some(STORE_REF)).unwrap().as_deref(), Some("sk-test"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(FILE)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store(dir.path(), None).unwrap();
        assert_eq!(resolve(dir.path(), Some(STORE_REF)).unwrap(), None);
        assert!(resolve(dir.path(), Some("sk-raw-key")).is_err());
        assert_eq!(resolve(dir.path(), None).unwrap(), None);
    }
}
