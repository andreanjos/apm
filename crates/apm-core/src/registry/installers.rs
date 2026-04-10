use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// A vendor installer app definition loaded from `installers.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerDefinition {
    pub key: String,
    pub name: String,
    pub vendor: String,
    pub app_paths: Vec<PathBuf>,
    pub download_url: String,
    pub homepage: String,
}

#[derive(Debug, Deserialize)]
struct RawInstallerDefinition {
    name: String,
    vendor: String,
    #[serde(default)]
    app_paths: Vec<PathBuf>,
    download_url: String,
    homepage: String,
}

#[derive(Debug, Deserialize)]
struct InstallersFile {
    #[serde(flatten)]
    installers: HashMap<String, RawInstallerDefinition>,
}

pub fn load_installers_toml(path: &Path) -> Result<HashMap<String, InstallerDefinition>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read installers file: {}", path.display()))?;
    let parsed: InstallersFile = toml::from_str(&raw).map_err(|e| {
        anyhow::anyhow!(
            "TOML parse error in {}:\n  {}\nHint: Fix the syntax error in the installers file.",
            path.display(),
            e
        )
    })?;

    Ok(parsed
        .installers
        .into_iter()
        .map(|(key, installer)| {
            (
                key.clone(),
                InstallerDefinition {
                    key,
                    name: installer.name,
                    vendor: installer.vendor,
                    app_paths: installer.app_paths,
                    download_url: installer.download_url,
                    homepage: installer.homepage,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_installers_assigns_table_key() {
        let temp_dir = std::env::temp_dir().join(format!(
            "apm-installers-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let path = temp_dir.join("installers.toml");

        std::fs::write(
            &path,
            r#"
[native-access]
name = "Native Access 2"
vendor = "Native Instruments"
app_paths = ["/Applications/Native Access.app"]
download_url = "https://www.native-instruments.com/en/specials/native-access/"
homepage = "https://www.native-instruments.com/"
"#,
        )
        .unwrap();

        let installers = load_installers_toml(&path).expect("installer file should load");
        let installer = installers
            .get("native-access")
            .expect("native-access installer should exist");

        assert_eq!(installer.key, "native-access");
        assert_eq!(installer.name, "Native Access 2");
        assert_eq!(installer.vendor, "Native Instruments");
        assert_eq!(
            installer.app_paths,
            vec![PathBuf::from("/Applications/Native Access.app")]
        );
    }
}
