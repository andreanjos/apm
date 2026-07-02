use std::path::PathBuf;

use chrono::Utc;
#[cfg(feature = "reqwest")]
use sha2::{Digest, Sha256};
#[cfg(feature = "reqwest")]
use std::io::{Read, Write};
#[cfg(feature = "reqwest")]
use std::net::TcpListener;
#[cfg(feature = "reqwest")]
use std::thread;
#[cfg(feature = "reqwest")]
use zip::write::SimpleFileOptions;

use super::*;
use crate::config::Config;
#[cfg(feature = "reqwest")]
use crate::engine::EngineEvent;
use crate::registry::PluginFormat;
use crate::state::{InstallOrigin, InstalledFormat, InstalledPlugin};

fn test_config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    write_test_registry(&config);
    (temp, config)
}

fn write_test_registry(config: &Config) {
    let plugins_dir = config.registries_cache_dir().join("official/plugins");
    std::fs::create_dir_all(&plugins_dir).expect("create registry plugins dir");
    std::fs::write(
        plugins_dir.join("direct-zip.toml"),
        r#"
slug = "direct-zip"
name = "Direct ZIP"
vendor = "Test Vendor"
version = "2.0.0"
description = "A direct zip install fixture"
category = "effects"
license = "MIT"

	[formats.vst3]
	url = "https://example.com/direct-zip.zip"
	sha256 = "manual"
	install_type = "zip"
	bundle_path = "DirectZip.vst3"

	[formats.au]
	url = "https://example.com/direct-zip-au.zip"
	sha256 = "manual"
	install_type = "zip"
	bundle_path = "DirectZip.component"
	"#,
    )
    .expect("write direct zip fixture");
    std::fs::write(
        plugins_dir.join("letters.toml"),
        r#"
slug = "letters"
name = "Letters"
vendor = "Test Vendor"
version = "beta"
description = "A non-semver fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "https://example.com/letters.zip"
sha256 = "manual"
install_type = "zip"
bundle_path = "Letters.vst3"
"#,
    )
    .expect("write letters fixture");
}

fn write_state(config: &Config, plugins: Vec<InstalledPlugin>) {
    crate::state::InstallState {
        version: 1,
        plugins,
    }
    .save(config)
    .expect("save state");
}

fn installed_plugin(
    name: &str,
    version: &str,
    pinned: bool,
    origin: InstallOrigin,
) -> InstalledPlugin {
    installed_plugin_with_formats(name, version, pinned, origin, vec![PluginFormat::Vst3])
}

fn installed_plugin_with_formats(
    name: &str,
    version: &str,
    pinned: bool,
    origin: InstallOrigin,
    formats: Vec<PluginFormat>,
) -> InstalledPlugin {
    InstalledPlugin {
        name: name.to_string(),
        version: version.to_string(),
        vendor: "Test Vendor".to_string(),
        formats: formats
            .into_iter()
            .map(|format| InstalledFormat {
                format,
                path: PathBuf::from(format!("/tmp/{name}.{format}")),
                sha256: String::new(),
            })
            .collect(),
        installed_at: Utc::now(),
        source: "official".to_string(),
        pinned,
        origin,
    }
}

#[test]
fn available_updates_classifies_actionable_pinned_and_external_updates() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![
            installed_plugin("direct-zip", "1.0.0", false, InstallOrigin::Apm),
            installed_plugin("direct-zip-pinned", "1.0.0", true, InstallOrigin::Apm),
            installed_plugin(
                "direct-zip-external",
                "1.0.0",
                false,
                InstallOrigin::External,
            ),
        ],
    );
    copy_registry_fixture(&config, "direct-zip", "direct-zip-pinned");
    copy_registry_fixture(&config, "direct-zip", "direct-zip-external");
    let engine = ApmEngine::new(config);

    let result = engine
        .available_updates(AvailableUpdatesRequest)
        .expect("updates should load");
    let AvailableUpdatesResult::Ready {
        updates,
        pinned_count,
        external_count,
        ..
    } = result
    else {
        panic!("registry should be populated");
    };

    assert_eq!(updates.len(), 3);
    assert_eq!(pinned_count, 1);
    assert_eq!(external_count, 1);
    assert!(updates.iter().any(|update| {
        update.slug == "direct-zip" && update.action == PackageUpdateAction::Installable
    }));
    assert!(updates.iter().any(|update| {
        update.slug == "direct-zip-pinned" && update.action == PackageUpdateAction::Pinned
    }));
    assert!(updates.iter().any(|update| {
        update.slug == "direct-zip-external" && update.action == PackageUpdateAction::External
    }));
}

#[test]
fn available_updates_counts_up_to_date_and_missing_packages() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![
            installed_plugin("direct-zip", "2.0.0", false, InstallOrigin::Apm),
            installed_plugin("missing", "1.0.0", false, InstallOrigin::Apm),
        ],
    );
    let engine = ApmEngine::new(config);

    let result = engine
        .available_updates(AvailableUpdatesRequest)
        .expect("updates should load");

    assert!(matches!(
        result,
        AvailableUpdatesResult::Ready {
            updates,
            up_to_date_count: 1,
            missing_count: 1,
            ..
        } if updates.is_empty()
    ));
}

#[test]
fn available_updates_uses_string_difference_for_non_semver_versions() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![installed_plugin(
            "letters",
            "alpha",
            false,
            InstallOrigin::Apm,
        )],
    );
    let engine = ApmEngine::new(config);

    let result = engine
        .available_updates(AvailableUpdatesRequest)
        .expect("updates should load");

    assert!(matches!(
        result,
        AvailableUpdatesResult::Ready { updates, .. }
            if updates.len() == 1
                && updates[0].installed_version == "alpha"
                && updates[0].available_version == "beta"
    ));
}

#[test]
fn available_updates_reports_empty_catalog() {
    let temp = tempfile::tempdir().expect("temp dir");
    let config = Config {
        data_dir: Some(temp.path().join("data")),
        cache_dir: Some(temp.path().join("cache")),
        ..Config::default()
    };
    write_state(
        &config,
        vec![installed_plugin(
            "direct-zip",
            "1.0.0",
            false,
            InstallOrigin::Apm,
        )],
    );
    let engine = ApmEngine::new(config);

    let result = engine
        .available_updates(AvailableUpdatesRequest)
        .expect("updates should load");

    assert_eq!(result, AvailableUpdatesResult::CatalogEmpty);
}

#[cfg(feature = "reqwest")]
#[test]
fn update_package_downloads_latest_direct_zip() {
    let (temp, config) = test_config();
    let archive = temp.path().join("direct.zip");
    write_zip(&archive, &["DirectZip.vst3/Contents/Info.plist"]);
    let archive_bytes = std::fs::read(&archive).expect("read archive");
    let sha256 = sha256_hex(&archive_bytes);
    let server = serve_once(archive_bytes);
    rewrite_direct_zip_source(&config, &server.url, &sha256);
    write_state(
        &config,
        vec![installed_plugin(
            "direct-zip",
            "1.0.0",
            false,
            InstallOrigin::Apm,
        )],
    );

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .update_package_with_dest_resolver(
            UpdatePackageRequest {
                slug: "direct-zip".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect("update should succeed");

    server.join();

    let UpdatePackageResult::Updated { update, package } = result else {
        panic!("expected updated result");
    };
    assert_eq!(update.installed_version, "1.0.0");
    assert_eq!(update.available_version, "2.0.0");
    assert_eq!(package.version, "2.0.0");
    assert!(install_root
        .join("vst3/DirectZip.vst3/Contents/Info.plist")
        .exists());
    assert_eq!(
        crate::state::InstallState::load(&config)
            .expect("state should load")
            .find("direct-zip")
            .expect("updated state")
            .version,
        "2.0.0"
    );
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadStarted {
                slug,
                format: PluginFormat::Vst3,
                ..
            } if slug == "direct-zip"
        )
    }));
}

#[cfg(feature = "reqwest")]
#[test]
fn update_package_refuses_partial_multi_format_update() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![installed_plugin_with_formats(
            "direct-zip",
            "1.0.0",
            false,
            InstallOrigin::Apm,
            vec![PluginFormat::Au, PluginFormat::Vst3],
        )],
    );
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .update_package(
            UpdatePackageRequest {
                slug: "direct-zip".to_string(),
                format: Some(PluginFormat::Vst3),
                ..UpdatePackageRequest::default()
            },
            &mut |event| events.push(event),
        )
        .expect("multi-format update should classify");

    assert!(events.is_empty());
    assert!(matches!(
        result,
        UpdatePackageResult::InstallUnavailable {
            result: InstallPackageResult::FormatRequired {
                available_formats,
                reason,
                ..
            },
            ..
        } if available_formats == vec![PluginFormat::Au, PluginFormat::Vst3]
            && reason.contains("update all tracked formats together")
    ));
    let state = crate::state::InstallState::load(&config).expect("state should load");
    let installed = state
        .find("direct-zip")
        .expect("install should remain tracked");
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(installed.formats.len(), 2);
}

#[cfg(feature = "reqwest")]
#[test]
fn update_package_downloads_all_tracked_multi_format_zips() {
    let (temp, config) = test_config();
    let au_archive = temp.path().join("direct-au.zip");
    let vst3_archive = temp.path().join("direct-vst3.zip");
    write_zip(&au_archive, &["DirectZip.component/Contents/Info.plist"]);
    write_zip(&vst3_archive, &["DirectZip.vst3/Contents/Info.plist"]);
    let au_bytes = std::fs::read(&au_archive).expect("read au archive");
    let vst3_bytes = std::fs::read(&vst3_archive).expect("read vst3 archive");
    let au_sha256 = sha256_hex(&au_bytes);
    let vst3_sha256 = sha256_hex(&vst3_bytes);
    let au_server = serve_once(au_bytes);
    let vst3_server = serve_once(vst3_bytes);
    rewrite_direct_zip_sources(
        &config,
        &vst3_server.url,
        &vst3_sha256,
        &au_server.url,
        &au_sha256,
    );
    write_state(
        &config,
        vec![installed_plugin_with_formats(
            "direct-zip",
            "1.0.0",
            false,
            InstallOrigin::Apm,
            vec![PluginFormat::Au, PluginFormat::Vst3],
        )],
    );

    let install_root = temp.path().join("install-root");
    let engine = ApmEngine::new(config.clone());
    let mut events = Vec::new();

    let result = engine
        .update_package_with_dest_resolver(
            UpdatePackageRequest {
                slug: "direct-zip".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut |event| events.push(event),
            |format, _scope| install_root.join(format.to_string().to_lowercase()),
        )
        .expect("multi-format update should succeed");

    au_server.join();
    vst3_server.join();

    let UpdatePackageResult::Updated { package, .. } = result else {
        panic!("expected updated result");
    };
    assert_eq!(package.version, "2.0.0");
    assert_eq!(package.formats.len(), 2);
    assert!(install_root
        .join("au/DirectZip.component/Contents/Info.plist")
        .exists());
    assert!(install_root
        .join("vst3/DirectZip.vst3/Contents/Info.plist")
        .exists());

    let state = crate::state::InstallState::load(&config).expect("state should load");
    let installed = state.find("direct-zip").expect("updated state");
    assert_eq!(installed.version, "2.0.0");
    assert_eq!(installed.formats.len(), 2);
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadStarted {
                slug,
                format: PluginFormat::Au,
                ..
            } if slug == "direct-zip"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallDownloadStarted {
                slug,
                format: PluginFormat::Vst3,
                ..
            } if slug == "direct-zip"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EngineEvent::InstallFinished {
                slug,
                installed_format_count: 2
            } if slug == "direct-zip"
        )
    }));
}

#[cfg(feature = "reqwest")]
#[test]
fn update_package_skips_pinned_and_external_installs() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![
            installed_plugin("direct-zip", "1.0.0", true, InstallOrigin::Apm),
            installed_plugin(
                "direct-zip-external",
                "1.0.0",
                false,
                InstallOrigin::External,
            ),
        ],
    );
    copy_registry_fixture(&config, "direct-zip", "direct-zip-external");
    let engine = ApmEngine::new(config);
    let mut events = Vec::new();

    let pinned = engine
        .update_package(
            UpdatePackageRequest {
                slug: "direct-zip".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut |event| events.push(event),
        )
        .expect("pinned update should classify");
    let external = engine
        .update_package(
            UpdatePackageRequest {
                slug: "direct-zip-external".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut |event| events.push(event),
        )
        .expect("external update should classify");

    assert!(matches!(pinned, UpdatePackageResult::Pinned { .. }));
    assert!(matches!(external, UpdatePackageResult::External { .. }));
    assert!(events.is_empty());
}

#[cfg(feature = "reqwest")]
#[test]
fn update_package_reports_up_to_date_and_missing_packages() {
    let (_temp, config) = test_config();
    write_state(
        &config,
        vec![installed_plugin(
            "direct-zip",
            "2.0.0",
            false,
            InstallOrigin::Apm,
        )],
    );
    let engine = ApmEngine::new(config);
    let mut noop = |_| {};

    let up_to_date = engine
        .update_package(
            UpdatePackageRequest {
                slug: "direct-zip".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut noop,
        )
        .expect("up-to-date update should classify");
    let not_installed = engine
        .update_package(
            UpdatePackageRequest {
                slug: "missing".to_string(),
                ..UpdatePackageRequest::default()
            },
            &mut noop,
        )
        .expect("missing update should classify");

    assert!(matches!(
        up_to_date,
        UpdatePackageResult::UpToDate {
            slug,
            version
        } if slug == "direct-zip" && version == "2.0.0"
    ));
    assert_eq!(
        not_installed,
        UpdatePackageResult::NotInstalled {
            slug: "missing".to_string()
        }
    );
}

fn copy_registry_fixture(config: &Config, source_slug: &str, target_slug: &str) {
    let plugins_dir = config.registries_cache_dir().join("official/plugins");
    let source = std::fs::read_to_string(plugins_dir.join(format!("{source_slug}.toml")))
        .expect("read source fixture");
    let cloned = source
        .replace(
            &format!("slug = \"{source_slug}\""),
            &format!("slug = \"{target_slug}\""),
        )
        .replace(
            "name = \"Direct ZIP\"",
            &format!("name = \"{target_slug}\""),
        );
    std::fs::write(plugins_dir.join(format!("{target_slug}.toml")), cloned)
        .expect("write cloned fixture");
}

#[cfg(feature = "reqwest")]
fn rewrite_direct_zip_source(config: &Config, url: &str, sha256: &str) {
    rewrite_direct_zip_sources(config, url, sha256, url, sha256);
}

#[cfg(feature = "reqwest")]
fn rewrite_direct_zip_sources(
    config: &Config,
    vst3_url: &str,
    vst3_sha256: &str,
    au_url: &str,
    au_sha256: &str,
) {
    let path = config
        .registries_cache_dir()
        .join("official/plugins/direct-zip.toml");
    let content = format!(
        r#"
slug = "direct-zip"
name = "Direct ZIP"
vendor = "Test Vendor"
version = "2.0.0"
description = "A direct zip install fixture"
category = "effects"
license = "MIT"

[formats.vst3]
url = "{vst3_url}"
sha256 = "{vst3_sha256}"
install_type = "zip"
bundle_path = "DirectZip.vst3"

[formats.au]
url = "{au_url}"
sha256 = "{au_sha256}"
install_type = "zip"
bundle_path = "DirectZip.component"
"#
    );
    std::fs::write(path, content).expect("write direct fixture");
}

#[cfg(feature = "reqwest")]
fn write_zip(path: &std::path::Path, entries: &[&str]) {
    let file = std::fs::File::create(path).expect("create zip");
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().unix_permissions(0o644);

    for entry in entries {
        writer.start_file(entry, options).expect("start zip file");
        writer.write_all(b"test").expect("write zip file");
    }

    writer.finish().expect("finish zip");
}

#[cfg(feature = "reqwest")]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(feature = "reqwest")]
struct TestServer {
    url: String,
    handle: thread::JoinHandle<()>,
}

#[cfg(feature = "reqwest")]
impl TestServer {
    fn join(self) {
        self.handle.join().expect("server thread should finish");
    }
}

#[cfg(feature = "reqwest")]
fn serve_once(body: Vec<u8>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let url = format!(
        "http://{}/direct.zip",
        listener.local_addr().expect("local addr")
    );
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("write header");
        stream.write_all(&body).expect("write body");
    });
    TestServer { url, handle }
}
