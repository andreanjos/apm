use std::{
    ffi::OsString,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub fn atomic_write(path: &Path, content: impl AsRef<[u8]>) -> Result<()> {
    let temp_path = atomic_write_temp_path(path);
    let write_result = (|| -> Result<()> {
        let mut file = File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file: {}", temp_path.display()))?;
        file.write_all(content.as_ref())
            .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to sync temp file: {}", temp_path.display()))?;
        fs::rename(&temp_path, path).with_context(|| {
            format!(
                "Failed to replace {} with {}",
                path.display(),
                temp_path.display()
            )
        })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn atomic_write_temp_path(path: &Path) -> PathBuf {
    let mut file_name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("apm-write"));
    file_name.push(".tmp");
    path.with_file_name(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_file_without_temp_leftover() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("state.json");
        let temp_path = atomic_write_temp_path(&path);

        atomic_write(&path, b"{\"ok\":true}").expect("atomic write");

        assert_eq!(
            fs::read_to_string(&path).expect("written file"),
            "{\"ok\":true}"
        );
        assert!(!temp_path.exists());
    }

    #[test]
    fn atomic_write_failure_keeps_existing_file() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("state.json");
        let temp_path = atomic_write_temp_path(&path);
        let original = b"existing state";
        fs::write(&path, original).expect("seed file");
        fs::create_dir(&temp_path).expect("block temp write path");

        let result = atomic_write(&path, b"new state");

        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("original file"), original.to_vec());
    }
}
