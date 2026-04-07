#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use assert_fs::TempDir;
use sha2::{Digest, Sha256};

pub struct CliTestEnv {
    pub config_home: TempDir,
    pub data_home: TempDir,
    pub cache_home: TempDir,
}

impl CliTestEnv {
    pub fn new() -> Self {
        Self {
            config_home: TempDir::new().unwrap(),
            data_home: TempDir::new().unwrap(),
            cache_home: TempDir::new().unwrap(),
        }
    }

    pub fn apply(&self, cmd: &mut std::process::Command) {
        cmd.env("XDG_CONFIG_HOME", self.config_home.path())
            .env("XDG_DATA_HOME", self.data_home.path())
            .env("XDG_CACHE_HOME", self.cache_home.path())
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");
    }
}

pub fn command(env: &CliTestEnv) -> std::process::Command {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let binary = std::env::var("CARGO_BIN_EXE_apm")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            manifest
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("target/debug/apm")
        });
    let mut command = std::process::Command::new(binary);
    env.apply(&mut command);
    command
}

pub fn read_to_string(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

pub fn test_plugin_archive_sha256(slug: &str) -> String {
    let bytes = plugin_archive_bytes(slug);
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

fn plugin_archive_bytes(slug: &str) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(&mut cursor);
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    let bundle_dir = format!("{slug}.component/");
    let contents_dir = format!("{bundle_dir}Contents/");
    archive.add_directory(&bundle_dir, options).unwrap();
    archive.add_directory(&contents_dir, options).unwrap();
    archive
        .start_file(format!("{contents_dir}Info.plist"), options)
        .unwrap();
    archive
        .write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleName</key><string>Test Plugin</string></dict></plist>"#,
        )
        .unwrap();
    archive.finish().unwrap();
    cursor.into_inner()
}
