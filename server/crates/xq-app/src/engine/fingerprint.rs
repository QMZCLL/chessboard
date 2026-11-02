//! 引擎程序与网络文件的内容校验值。
//!
//! 分析结果按引擎身份缓存；只用 UCI 报告的名称与网络文件的名称、大小时，原位升级的引擎程序或
//! 大小恰好相同的另一份网络文件会复用旧的结果。校验值按文件内容计算（SHA-256），
//! 同一进程中按路径、大小与修改时间记住，重启引擎时不必重新读取整份文件（网络文件约 46 MB）。

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::SystemTime;

use sha2::Digest;
use sha2::Sha256;

type Stamp = (u64, Option<SystemTime>);

fn known() -> &'static Mutex<HashMap<PathBuf, (Stamp, String)>> {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, (Stamp, String)>>> = OnceLock::new();
    KNOWN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 一个文件内容的 SHA-256（十六进制）；文件不存在或读取失败时返回说明，保证与任何内容的校验值都不同。
pub(crate) fn file_digest(path: &Path) -> String {
    let stamp = match std::fs::metadata(path) {
        Ok(meta) => (meta.len(), meta.modified().ok()),
        Err(e) => return format!("unreadable:{}", e.kind()),
    };
    let mut cache = known().lock().unwrap_or_else(|p| p.into_inner());
    if let Some((s, digest)) = cache.get(path)
        && *s == stamp
    {
        return digest.clone();
    }
    let digest = match hash(path) {
        Ok(d) => d,
        Err(e) => return format!("unreadable:{}", e.kind()),
    };
    cache.insert(path.to_path_buf(), (stamp, digest.clone()));
    digest
}

fn hash(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// 引擎身份的校验值：引擎程序与网络文件（没有指定时为引擎默认）的内容合在一起，取前 16 位。
pub(crate) fn engine_fingerprint(binary: &Path, nnue: Option<&Path>) -> String {
    let text = format!("{}|{}", file_digest(binary), nnue.map_or_else(|| "default".to_string(), file_digest));
    Sha256::digest(text.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_content_gives_a_different_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let (bin, net) = (dir.path().join("engine"), dir.path().join("net.nnue"));
        std::fs::write(&bin, b"engine-v1").unwrap();
        std::fs::write(&net, b"weights-a").unwrap();
        let first = engine_fingerprint(&bin, Some(&net));
        assert_eq!(first.len(), 16);
        assert_eq!(engine_fingerprint(&bin, Some(&net)), first, "内容不变时校验值不变");
        // 同名、同样大小、内容不同：按内容区分（清掉记住的结果，确认不是按大小或修改时间）
        std::fs::write(&net, b"weights-b").unwrap();
        known().lock().unwrap().clear();
        let second = engine_fingerprint(&bin, Some(&net));
        assert_ne!(second, first, "网络文件内容不同");
        // 大小改变时不用记住的结果
        std::fs::write(&bin, b"engine-v10").unwrap();
        assert_ne!(engine_fingerprint(&bin, Some(&net)), second, "引擎程序原位升级");
        assert_ne!(engine_fingerprint(&bin, None), engine_fingerprint(&bin, Some(&net)), "没有指定网络文件时与指定时不同");
        assert!(file_digest(&dir.path().join("missing")).starts_with("unreadable:"));
    }
}
