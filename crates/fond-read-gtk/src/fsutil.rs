use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write via a temp file in the same directory, fsync, then rename over `path`, keeping the
/// previous contents as `<name>.bak`. A crash or full disk leaves either the old or the new file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = sibling(path, ".tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    if path.exists() {
        let _ = fs::copy(path, sibling(path, ".bak"));
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// What reading a persisted file found.
pub enum Loaded<T> {
    Missing,
    Ok(T),
    /// The file existed but didn't parse; it has been moved aside to `quarantined`.
    Corrupt {
        quarantined: PathBuf,
        recovered_from_backup: Option<T>,
    },
}

/// Read and parse `path`. A file that fails to parse is renamed to `<name>.corrupt-<unix time>`
/// instead of being left to be overwritten by the next save, and `<name>.bak` is tried as a
/// fallback.
pub fn load_checked<T>(path: &Path, parse: impl Fn(&str) -> Option<T>) -> Loaded<T> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(_) => String::new(),
    };
    if let Some(v) = parse(&text) {
        return Loaded::Ok(v);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let quarantined = sibling(path, &format!(".corrupt-{stamp}"));
    let _ = fs::rename(path, &quarantined);
    let recovered_from_backup = fs::read_to_string(sibling(path, ".bak"))
        .ok()
        .and_then(|t| parse(&t));
    Loaded::Corrupt {
        quarantined,
        recovered_from_backup,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pereplyot-fsutil-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn parse_num(s: &str) -> Option<u32> {
        s.trim().parse().ok()
    }

    #[test]
    fn atomic_write_keeps_backup_and_leaves_no_tmp() {
        let d = dir("atomic");
        let p = d.join("a.json");
        write_atomic(&p, b"1").unwrap();
        write_atomic(&p, b"2").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "2");
        assert_eq!(fs::read_to_string(sibling(&p, ".bak")).unwrap(), "1");
        assert!(!sibling(&p, ".tmp").exists());
    }

    #[test]
    fn missing_file_is_missing() {
        let d = dir("missing");
        assert!(matches!(
            load_checked(&d.join("nope"), parse_num),
            Loaded::Missing
        ));
    }

    #[test]
    fn corrupt_file_is_quarantined_and_backup_used() {
        let d = dir("corrupt");
        let p = d.join("a.json");
        write_atomic(&p, b"7").unwrap();
        write_atomic(&p, b"8").unwrap();
        fs::write(&p, b"{truncated").unwrap();
        match load_checked(&p, parse_num) {
            Loaded::Corrupt {
                quarantined,
                recovered_from_backup,
            } => {
                assert!(quarantined.exists());
                assert!(!p.exists());
                assert_eq!(recovered_from_backup, Some(7));
            }
            _ => panic!("expected corrupt"),
        }
    }
}
