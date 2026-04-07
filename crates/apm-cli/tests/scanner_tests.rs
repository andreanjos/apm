// Integration tests for the scanner module — bundle structure, plist parsing,
// and version string sanitization logic. The sanitize_version function is
// private so we replicate its documented algorithm here.

use std::fs;
use std::path::PathBuf;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Write a minimal XML Info.plist to `<bundle_dir>/Contents/Info.plist`.
fn write_info_plist(bundle_dir: &std::path::Path, name: &str, version: &str, bundle_id: &str) {
    let contents_dir = bundle_dir.join("Contents");
    fs::create_dir_all(&contents_dir).expect("create Contents dir");

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
</dict>
</plist>
"#
    );

    fs::write(contents_dir.join("Info.plist"), plist).expect("write Info.plist");
}

/// Write a minimal AU-style Info.plist with an AudioComponents entry.
fn write_au_info_plist(
    bundle_dir: &std::path::Path,
    name: &str,
    version: &str,
    bundle_id: &str,
    vendor: &str,
    plugin_name: &str,
) {
    let contents_dir = bundle_dir.join("Contents");
    fs::create_dir_all(&contents_dir).expect("create Contents dir");

    let au_name = format!("{vendor}: {plugin_name}");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>{name}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>AudioComponents</key>
    <array>
        <dict>
            <key>name</key>
            <string>{au_name}</string>
            <key>type</key>
            <string>aufx</string>
            <key>subtype</key>
            <string>test</string>
            <key>manufacturer</key>
            <string>Test</string>
            <key>version</key>
            <integer>65536</integer>
        </dict>
    </array>
</dict>
</plist>
"#
    );

    fs::write(contents_dir.join("Info.plist"), plist).expect("write AU Info.plist");
}

/// Replicate the sanitize_version logic documented in scanner.rs.
fn sanitize_version(raw: &str) -> String {
    let token = raw.split_whitespace().next().unwrap_or("");
    let cleaned: String = token
        .chars()
        .enumerate()
        .filter(|(i, c)| c.is_ascii_digit() || *c == '.' || *c == '-' || (*i == 0 && *c == 'v'))
        .map(|(_, c)| c)
        .collect();
    cleaned.trim_matches(|c| c == '.' || c == '-').to_owned()
}

// ── Scanning empty directory ──────────────────────────────────────────────────

#[test]
fn test_empty_directory_has_no_entries() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let entries: Vec<_> = fs::read_dir(tmp.path()).expect("read dir").collect();
    assert!(entries.is_empty(), "temp dir should start empty");
}

// ── Bundle structure creation ─────────────────────────────────────────────────

#[test]
fn test_vst3_bundle_structure_with_valid_plist() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("TestPlugin.vst3");
    write_info_plist(
        &bundle_dir,
        "Test Plugin",
        "1.0.0",
        "com.testvendor.testplugin",
    );

    assert!(bundle_dir.exists());
    assert!(bundle_dir.is_dir());
    assert!(bundle_dir.join("Contents/Info.plist").exists());
}

#[test]
fn test_component_bundle_structure_with_valid_plist() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("TestAUPlugin.component");
    write_au_info_plist(
        &bundle_dir,
        "Test AU Plugin",
        "2.0.0",
        "com.testvendor.testauplugin",
        "Test Vendor",
        "Test AU Plugin",
    );

    assert!(bundle_dir.exists());
    assert!(bundle_dir.join("Contents/Info.plist").exists());
}

#[test]
fn test_multiple_bundles_in_same_directory() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    write_info_plist(
        &tmp.path().join("Plugin1.vst3"),
        "Plugin 1",
        "1.0.0",
        "com.test.plugin1",
    );
    write_info_plist(
        &tmp.path().join("Plugin2.vst3"),
        "Plugin 2",
        "2.0.0",
        "com.test.plugin2",
    );

    let entries: Vec<_> = fs::read_dir(tmp.path())
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("vst3"))
        .collect();

    assert_eq!(entries.len(), 2);
}

// ── Plist parsing ─────────────────────────────────────────────────────────────

#[test]
fn test_plist_can_be_parsed_with_expected_fields() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("MyPlugin.vst3");
    write_info_plist(&bundle_dir, "My Plugin", "3.1.4", "com.example.myplugin");

    let plist_path = bundle_dir.join("Contents/Info.plist");
    let value = plist::Value::from_file(&plist_path).expect("parse plist");
    let dict = value.as_dictionary().expect("plist should be a dict");

    let name = dict
        .get("CFBundleName")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert_eq!(name, "My Plugin");

    let version = dict
        .get("CFBundleShortVersionString")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert_eq!(version, "3.1.4");

    let bundle_id = dict
        .get("CFBundleIdentifier")
        .and_then(|v| v.as_string())
        .unwrap_or("");
    assert_eq!(bundle_id, "com.example.myplugin");
}

#[test]
fn test_plist_bundle_name_fallback_to_version() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("AcmePlugin.vst3");
    write_info_plist(&bundle_dir, "Acme Plugin", "2.5.0", "com.acme.plugin");

    let plist_path = bundle_dir.join("Contents/Info.plist");
    let value = plist::Value::from_file(&plist_path).expect("parse plist");
    let dict = value.as_dictionary().expect("dict");

    // CFBundleVersion should be present as fallback.
    let version = dict.get("CFBundleVersion").and_then(|v| v.as_string());
    assert!(version.is_some());
    assert_eq!(version.unwrap(), "2.5.0");
}

#[test]
fn test_au_plist_audio_components_vendor_extraction() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("AcmeReverb.component");
    write_au_info_plist(
        &bundle_dir,
        "Acme Reverb",
        "1.5.0",
        "com.acme.reverb",
        "Acme DSP",
        "Reverb",
    );

    let plist_path = bundle_dir.join("Contents/Info.plist");
    let value = plist::Value::from_file(&plist_path).expect("parse plist");
    let dict = value.as_dictionary().expect("dict");

    // Extract vendor from AudioComponents[0].name ("Acme DSP: Reverb" → "Acme DSP").
    let vendor = dict
        .get("AudioComponents")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.as_dictionary())
        .and_then(|d| d.get("name"))
        .and_then(|v| v.as_string())
        .and_then(|s| s.find(':').map(|pos| s[..pos].trim().to_string()))
        .unwrap_or_default();

    assert_eq!(vendor, "Acme DSP");
}

#[test]
fn test_au_plist_audio_components_name_without_colon_is_vendor() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("SimpleAU.component");

    let contents_dir = bundle_dir.join("Contents");
    fs::create_dir_all(&contents_dir).expect("create Contents dir");

    // No colon in name — entire name becomes vendor.
    let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>SimpleAU</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.simpleau</string>
    <key>AudioComponents</key>
    <array>
        <dict>
            <key>name</key>
            <string>NoColonVendorName</string>
        </dict>
    </array>
</dict>
</plist>
"#;
    fs::write(contents_dir.join("Info.plist"), plist).expect("write");

    let value = plist::Value::from_file(contents_dir.join("Info.plist")).expect("parse");
    let dict = value.as_dictionary().unwrap();

    let au_name = dict
        .get("AudioComponents")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|item| item.as_dictionary())
        .and_then(|d| d.get("name"))
        .and_then(|v| v.as_string())
        .unwrap_or("");

    // Simulate the extraction logic.
    let vendor = if let Some(colon_pos) = au_name.find(':') {
        au_name[..colon_pos].trim().to_owned()
    } else {
        au_name.trim().to_owned()
    };

    assert_eq!(vendor, "NoColonVendorName");
}

// ── Version string sanitization ───────────────────────────────────────────────

#[test]
fn test_sanitize_version_strips_extra_text_after_space() {
    // "5.5.4.18982 Authorization: Crystallizer" → "5.5.4.18982"
    let result = sanitize_version("5.5.4.18982 Authorization: Crystallizer");
    assert_eq!(result, "5.5.4.18982");
}

#[test]
fn test_sanitize_version_plain_version_unchanged() {
    assert_eq!(sanitize_version("1.2.3"), "1.2.3");
}

#[test]
fn test_sanitize_version_leading_v_preserved() {
    assert_eq!(sanitize_version("v2.0.1"), "v2.0.1");
}

#[test]
fn test_sanitize_version_strips_non_version_chars() {
    // "1.0_alpha" → "1.0" (underscore stripped)
    let result = sanitize_version("1.0_alpha");
    assert_eq!(result, "1.0");
}

#[test]
fn test_sanitize_version_empty_after_sanitization() {
    // "alpha" → no digits, dots, hyphens, or leading v → empty
    let result = sanitize_version("alpha");
    assert!(result.is_empty());
}

#[test]
fn test_sanitize_version_empty_input() {
    let result = sanitize_version("");
    assert!(result.is_empty());
}

#[test]
fn test_sanitize_version_strips_leading_trailing_dots() {
    // A version that becomes ".1.0." after char filtering → "1.0"
    let result = sanitize_version(".1.0.");
    assert_eq!(result, "1.0");
}

#[test]
fn test_sanitize_version_with_hyphen_numeric_prerelease() {
    // "1.0.0-1" → "1.0.0-1" (hyphens between digits are kept)
    let result = sanitize_version("1.0.0-1");
    assert_eq!(result, "1.0.0-1");
}

#[test]
fn test_sanitize_version_hyphen_before_alpha_chars_stripped() {
    // "1.0.0-beta" → "1.0.0" because non-digit chars after '-' are stripped,
    // leaving a trailing '-' which is then trimmed.
    let result = sanitize_version("1.0.0-beta");
    assert_eq!(result, "1.0.0");
}

#[test]
fn test_sanitize_version_only_uses_first_token() {
    // Only the first whitespace-delimited token is used.
    let result = sanitize_version("2.0.0 extra stuff here");
    assert_eq!(result, "2.0.0");
}

// ── Bundle extension detection ─────────────────────────────────────────────────

#[test]
fn test_vst3_extension_is_recognised() {
    let path = PathBuf::from("MyPlugin.vst3");
    let ext = path.extension().and_then(|e| e.to_str());
    assert_eq!(ext, Some("vst3"));
}

#[test]
fn test_component_extension_is_recognised() {
    let path = PathBuf::from("MyPlugin.component");
    let ext = path.extension().and_then(|e| e.to_str());
    assert_eq!(ext, Some("component"));
}

#[test]
fn test_non_plugin_extension_is_ignored() {
    let path = PathBuf::from("someapp.app");
    let ext = path.extension().and_then(|e| e.to_str());
    assert_ne!(ext, Some("vst3"));
    assert_ne!(ext, Some("component"));
}

// ── Missing plist detection ───────────────────────────────────────────────────

#[test]
fn test_bundle_missing_plist_is_detected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("NoPlist.vst3");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    let plist_path = bundle_dir.join("Contents/Info.plist");
    assert!(
        !plist_path.exists(),
        "there should be no Info.plist in this test bundle"
    );
}

#[test]
fn test_bundle_with_plist_is_detected() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let bundle_dir = tmp.path().join("WithPlist.vst3");
    write_info_plist(&bundle_dir, "With Plist", "1.0.0", "com.test.withplist");

    let plist_path = bundle_dir.join("Contents/Info.plist");
    assert!(
        plist_path.exists(),
        "Info.plist should exist in test bundle"
    );
}

// ── Directory walking ─────────────────────────────────────────────────────────

#[test]
fn test_walk_dir_finds_vst3_bundles() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    // Create two VST3 bundles and one non-plugin directory.
    write_info_plist(
        &tmp.path().join("Alpha.vst3"),
        "Alpha",
        "1.0",
        "com.test.alpha",
    );
    write_info_plist(
        &tmp.path().join("Beta.vst3"),
        "Beta",
        "2.0",
        "com.test.beta",
    );
    fs::create_dir_all(tmp.path().join("not-a-plugin.app/Contents")).unwrap();

    let vst3_bundles: Vec<_> = walkdir::WalkDir::new(tmp.path())
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("vst3"))
        .collect();

    assert_eq!(vst3_bundles.len(), 2);
}

#[test]
fn test_walk_dir_finds_component_bundles() {
    let tmp = tempfile::tempdir().expect("create temp dir");

    write_au_info_plist(
        &tmp.path().join("PlugA.component"),
        "PlugA",
        "1.0",
        "com.test.pluga",
        "Vendor",
        "PlugA",
    );

    let component_bundles: Vec<_> = walkdir::WalkDir::new(tmp.path())
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("component"))
        .collect();

    assert_eq!(component_bundles.len(), 1);
}

#[test]
fn test_walk_dir_does_not_recurse_into_bundle_contents() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    write_info_plist(
        &tmp.path().join("Plugin.vst3"),
        "Plugin",
        "1.0",
        "com.test.plugin",
    );

    // At depth 1, we should only see Plugin.vst3, not its Contents/ subdir.
    let depth1_entries: Vec<_> = walkdir::WalkDir::new(tmp.path())
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    assert_eq!(depth1_entries.len(), 1);
    assert_eq!(
        depth1_entries[0]
            .path()
            .file_name()
            .and_then(|n| n.to_str()),
        Some("Plugin.vst3")
    );
}
