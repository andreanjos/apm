// Portable setup encoding and decoding for shareable apm1:// strings.
//
// Pipeline: PortableSetup -> JSON -> DEFLATE -> base64url -> "apm1://{payload}"
// Reverse:  strip prefix  -> base64url -> INFLATE -> JSON -> PortableSetup

use std::io::{Read as IoRead, Write as IoWrite};

use anyhow::{bail, Context, Result};
use base64::engine::{general_purpose::URL_SAFE_NO_PAD, Engine};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use apm_core::config::{Config, InstallScope};
use apm_core::state::InstallState;

// ── Constants ────────────────────────────────────────────────────────────────

const SCHEME_PREFIX: &str = "apm1://";
const DEFAULT_REGISTRY_URL: &str = "https://github.com/apm-pm/registry";

// ── Data Model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortableSetup {
    /// Schema version (always 1 for apm1://).
    pub v: u8,
    /// Plugins list.
    pub p: Vec<PortablePlugin>,
    /// Non-default sources: (name, url) pairs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub s: Vec<(String, String)>,
    /// Config overrides (only non-default values).
    #[serde(default)]
    pub c: PortableConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortablePlugin {
    /// Plugin slug.
    pub n: String,
    /// Version string.
    pub v: String,
    /// Pinned flag (omitted when false).
    #[serde(default, skip_serializing_if = "is_false")]
    pub p: bool,
    /// Source name.
    pub s: String,
}

fn is_false(b: &bool) -> bool {
    !b
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PortableConfig {
    /// Install scope (only present when "system"; omitted for default "user").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sc: Option<String>,
    /// Registry URL (only present when non-default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reg: Option<String>,
}

// ── Import Preview ───────────────────────────────────────────────────────────

pub struct ImportPreview {
    /// Plugins to install: (slug, version, pinned).
    pub to_install: Vec<(String, String, bool)>,
    /// Plugins already at the correct version.
    pub to_skip_same: Vec<String>,
    /// Plugins where the installed version is newer: (slug, import_ver, installed_ver).
    pub to_skip_newer: Vec<(String, String, String)>,
    /// Plugins already installed that need to be pinned.
    pub to_pin: Vec<String>,
    /// Plugins already installed that need to be unpinned.
    pub to_unpin: Vec<String>,
    /// Sources to add: (name, url).
    pub to_add_sources: Vec<(String, String)>,
    /// Sources with name match but different URL: (name, import_url, existing_url).
    pub source_url_mismatches: Vec<(String, String, String)>,
    /// Human-readable config differences.
    pub config_changes: Vec<String>,
}

// ── Encode / Decode ──────────────────────────────────────────────────────────

/// Encode a PortableSetup into an apm1:// string.
pub fn encode(setup: &PortableSetup) -> Result<String> {
    // Step 1: Compact JSON
    let json = serde_json::to_vec(setup).context("Failed to serialize portable setup to JSON")?;

    // Step 2: DEFLATE compress
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(&json)
        .context("Failed to compress portable setup")?;
    let compressed = encoder.finish().context("Failed to finalize compression")?;

    // Step 3: Base64url encode (no padding)
    let b64 = URL_SAFE_NO_PAD.encode(&compressed);

    Ok(format!("{SCHEME_PREFIX}{b64}"))
}

/// Decode an apm1:// string back into a PortableSetup.
pub fn decode(input: &str) -> Result<PortableSetup> {
    let payload = input
        .strip_prefix(SCHEME_PREFIX)
        .ok_or_else(|| anyhow::anyhow!("Invalid portable setup string: missing 'apm1://' prefix"))?;

    // Step 1: Base64url decode
    let compressed = URL_SAFE_NO_PAD
        .decode(payload)
        .context("Failed to decode base64 payload")?;

    // Step 2: INFLATE decompress
    let mut decoder = DeflateDecoder::new(&compressed[..]);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .context("Failed to decompress portable setup")?;

    // Step 3: Parse JSON
    let setup: PortableSetup =
        serde_json::from_slice(&json).context("Failed to parse portable setup JSON")?;

    // Validate version
    if setup.v != 1 {
        bail!(
            "Unsupported portable setup version {}. Hint: Upgrade apm.",
            setup.v
        );
    }

    Ok(setup)
}

// ── Conversion ───────────────────────────────────────────────────────────────

/// Build a PortableSetup from the current install state and config.
pub fn from_state_and_config(state: &InstallState, config: &Config) -> PortableSetup {
    let plugins = state
        .plugins
        .iter()
        .map(|p| PortablePlugin {
            n: p.name.clone(),
            v: p.version.clone(),
            p: p.pinned,
            s: p.source.clone(),
        })
        .collect();

    let sources = config
        .sources
        .iter()
        .map(|entry| (entry.name.clone(), entry.url.clone()))
        .collect();

    let sc = match config.install_scope {
        InstallScope::System => Some("system".to_string()),
        InstallScope::User => None,
    };

    let reg = if config.default_registry_url != DEFAULT_REGISTRY_URL {
        Some(config.default_registry_url.clone())
    } else {
        None
    };

    PortableSetup {
        v: 1,
        p: plugins,
        s: sources,
        c: PortableConfig { sc, reg },
    }
}

/// Build a preview of what importing a portable setup would do.
pub fn build_preview(
    setup: &PortableSetup,
    state: &InstallState,
    config: &Config,
) -> ImportPreview {
    let mut preview = ImportPreview {
        to_install: Vec::new(),
        to_skip_same: Vec::new(),
        to_skip_newer: Vec::new(),
        to_pin: Vec::new(),
        to_unpin: Vec::new(),
        to_add_sources: Vec::new(),
        source_url_mismatches: Vec::new(),
        config_changes: Vec::new(),
    };

    // Categorize each plugin
    for plugin in &setup.p {
        if let Some(installed) = state.find(&plugin.n) {
            if installed.version == plugin.v {
                // Same version installed
                preview.to_skip_same.push(plugin.n.clone());
                // Check pin status changes
                if plugin.p && !installed.pinned {
                    preview.to_pin.push(plugin.n.clone());
                } else if !plugin.p && installed.pinned {
                    preview.to_unpin.push(plugin.n.clone());
                }
            } else {
                // Different version — compare with semver
                let import_ver = semver::Version::parse(&plugin.v);
                let installed_ver = semver::Version::parse(&installed.version);

                match (import_ver, installed_ver) {
                    (Ok(imp), Ok(inst)) if inst > imp => {
                        // Installed is newer — skip
                        preview.to_skip_newer.push((
                            plugin.n.clone(),
                            plugin.v.clone(),
                            installed.version.clone(),
                        ));
                    }
                    _ => {
                        // Installed is older, or parse failed — install
                        preview
                            .to_install
                            .push((plugin.n.clone(), plugin.v.clone(), plugin.p));
                    }
                }
            }
        } else {
            // Not installed — add to install list
            preview
                .to_install
                .push((plugin.n.clone(), plugin.v.clone(), plugin.p));
        }
    }

    // Categorize sources
    for (name, url) in &setup.s {
        if let Some(existing) = config.sources.iter().find(|e| e.name == *name) {
            if existing.url != *url {
                preview.source_url_mismatches.push((
                    name.clone(),
                    url.clone(),
                    existing.url.clone(),
                ));
            }
            // If URL matches, skip silently.
        } else {
            preview
                .to_add_sources
                .push((name.clone(), url.clone()));
        }
    }

    // Config changes
    let current_scope = match config.install_scope {
        InstallScope::User => "user",
        InstallScope::System => "system",
    };
    let import_scope = setup.c.sc.as_deref().unwrap_or("user");
    if current_scope != import_scope {
        preview.config_changes.push(format!(
            "install_scope: {} -> {}",
            current_scope, import_scope
        ));
    }

    let import_reg = setup
        .c
        .reg
        .as_deref()
        .unwrap_or(DEFAULT_REGISTRY_URL);
    if config.default_registry_url != import_reg {
        preview.config_changes.push(format!(
            "registry_url: {} -> {}",
            config.default_registry_url, import_reg
        ));
    }

    preview
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use apm_core::config::{Config, InstallScope, SourceEntry};
    use apm_core::state::{InstallState, InstalledPlugin};
    use chrono::Utc;

    fn sample_setup() -> PortableSetup {
        PortableSetup {
            v: 1,
            p: vec![
                PortablePlugin {
                    n: "vital".to_string(),
                    v: "1.5.5".to_string(),
                    p: true,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "surge-xt".to_string(),
                    v: "1.3.1".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "dexed".to_string(),
                    v: "0.9.7".to_string(),
                    p: false,
                    s: "community".to_string(),
                },
            ],
            s: vec![(
                "community".to_string(),
                "https://github.com/example/community-registry".to_string(),
            )],
            c: PortableConfig {
                sc: Some("system".to_string()),
                reg: Some("https://custom.registry.example.com".to_string()),
            },
        }
    }

    #[test]
    fn test_encode_produces_apm1_prefix() {
        let setup = sample_setup();
        let encoded = encode(&setup).unwrap();
        assert!(
            encoded.starts_with("apm1://"),
            "Encoded string should start with 'apm1://', got: {}",
            &encoded[..encoded.len().min(20)]
        );
    }

    #[test]
    fn test_round_trip_full() {
        let setup = sample_setup();
        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(setup, decoded);
    }

    #[test]
    fn test_round_trip_empty() {
        let setup = PortableSetup {
            v: 1,
            p: vec![],
            s: vec![],
            c: PortableConfig::default(),
        };
        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(setup, decoded);
    }

    #[test]
    fn test_decode_rejects_missing_prefix() {
        let result = decode("garbage-string");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("apm1://"),
            "Error should mention 'apm1://', got: {err}"
        );
    }

    #[test]
    fn test_decode_rejects_unsupported_version() {
        // Manually build a v=2 payload by creating a modified setup
        let setup = PortableSetup {
            v: 2,
            p: vec![],
            s: vec![],
            c: PortableConfig::default(),
        };
        // Encode bypassing the version check (encode doesn't validate v)
        let json = serde_json::to_vec(&setup).unwrap();
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&json).unwrap();
        let compressed = encoder.finish().unwrap();
        let b64 = URL_SAFE_NO_PAD.encode(&compressed);
        let encoded = format!("apm1://{b64}");

        let result = decode(&encoded);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unsupported"),
            "Error should mention 'Unsupported', got: {err}"
        );
        assert!(
            err.contains("Upgrade"),
            "Error should mention 'Upgrade', got: {err}"
        );
    }

    #[test]
    fn test_size_estimate_15_plugins() {
        let slugs = [
            "vital",
            "surge-xt",
            "dexed",
            "helm",
            "odin2",
            "diva",
            "serum",
            "massive-x",
            "pigments",
            "phaseplant",
            "spire",
            "sylenth1",
            "analog-lab",
            "omnisphere",
            "kontakt",
        ];
        let plugins: Vec<PortablePlugin> = slugs
            .iter()
            .map(|s| PortablePlugin {
                n: s.to_string(),
                v: "1.0.0".to_string(),
                p: false,
                s: "official".to_string(),
            })
            .collect();
        let setup = PortableSetup {
            v: 1,
            p: plugins,
            s: vec![
                (
                    "community".to_string(),
                    "https://github.com/example/community".to_string(),
                ),
                (
                    "custom".to_string(),
                    "https://github.com/example/custom".to_string(),
                ),
            ],
            c: PortableConfig::default(),
        };

        let encoded = encode(&setup).unwrap();
        assert!(
            encoded.len() < 500,
            "Encoded string should be under 500 chars, got {} chars",
            encoded.len()
        );
    }

    #[test]
    fn test_pinned_false_omitted() {
        let setup = PortableSetup {
            v: 1,
            p: vec![PortablePlugin {
                n: "test-plugin".to_string(),
                v: "1.0.0".to_string(),
                p: false,
                s: "official".to_string(),
            }],
            s: vec![],
            c: PortableConfig::default(),
        };

        // Serialize to JSON directly to inspect
        let json = serde_json::to_string(&setup).unwrap();
        // The "p" key for the plugin's pinned field should not appear
        // We check the plugin object specifically
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let plugin = &parsed["p"][0];
        assert!(
            plugin.get("p").is_none(),
            "pinned=false should be omitted from JSON, got: {json}"
        );
    }

    #[test]
    fn test_default_config_omitted() {
        let setup = PortableSetup {
            v: 1,
            p: vec![],
            s: vec![],
            c: PortableConfig {
                sc: None,
                reg: None,
            },
        };

        let json = serde_json::to_string(&setup).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let config = &parsed["c"];
        assert!(
            config.get("sc").is_none(),
            "default scope should be omitted, got: {json}"
        );
        assert!(
            config.get("reg").is_none(),
            "default registry should be omitted, got: {json}"
        );
    }

    #[test]
    fn test_non_default_config_preserved() {
        let setup = PortableSetup {
            v: 1,
            p: vec![],
            s: vec![],
            c: PortableConfig {
                sc: Some("system".to_string()),
                reg: Some("https://custom.example.com".to_string()),
            },
        };

        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded.c.sc, Some("system".to_string()));
        assert_eq!(
            decoded.c.reg,
            Some("https://custom.example.com".to_string())
        );
    }

    #[test]
    fn test_build_preview_categorizes_correctly() {
        // Create a setup that imports 4 plugins:
        // - "vital" 1.5.5 pinned (already installed at 1.5.5, not pinned -> to_skip_same + to_pin)
        // - "surge-xt" 1.2.0 (installed at 1.3.1 which is newer -> to_skip_newer)
        // - "dexed" 0.9.7 (not installed -> to_install)
        // - "helm" 1.0.0 (installed at 0.9.0 which is older -> to_install)
        let setup = PortableSetup {
            v: 1,
            p: vec![
                PortablePlugin {
                    n: "vital".to_string(),
                    v: "1.5.5".to_string(),
                    p: true,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "surge-xt".to_string(),
                    v: "1.2.0".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "dexed".to_string(),
                    v: "0.9.7".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "helm".to_string(),
                    v: "1.0.0".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
            ],
            s: vec![
                (
                    "community".to_string(),
                    "https://github.com/example/community".to_string(),
                ),
                (
                    "existing-diff".to_string(),
                    "https://new-url.example.com".to_string(),
                ),
            ],
            c: PortableConfig {
                sc: Some("system".to_string()),
                reg: None,
            },
        };

        let state = InstallState {
            version: 1,
            plugins: vec![
                InstalledPlugin {
                    name: "vital".to_string(),
                    version: "1.5.5".to_string(),
                    vendor: "Vital Audio".to_string(),
                    formats: vec![],
                    installed_at: Utc::now(),
                    source: "official".to_string(),
                    pinned: false,
                },
                InstalledPlugin {
                    name: "surge-xt".to_string(),
                    version: "1.3.1".to_string(),
                    vendor: "Surge Synth Team".to_string(),
                    formats: vec![],
                    installed_at: Utc::now(),
                    source: "official".to_string(),
                    pinned: false,
                },
                InstalledPlugin {
                    name: "helm".to_string(),
                    version: "0.9.0".to_string(),
                    vendor: "Tytel".to_string(),
                    formats: vec![],
                    installed_at: Utc::now(),
                    source: "official".to_string(),
                    pinned: false,
                },
            ],
        };

        let config = Config {
            default_registry_url: "https://github.com/apm-pm/registry".to_string(),
            install_scope: InstallScope::User,
            data_dir: None,
            cache_dir: None,
            sources: vec![SourceEntry {
                name: "existing-diff".to_string(),
                url: "https://old-url.example.com".to_string(),
            }],
        };

        let preview = build_preview(&setup, &state, &config);

        // dexed (not installed) + helm (older version installed)
        assert_eq!(preview.to_install.len(), 2, "to_install: expected 2");
        assert!(preview.to_install.iter().any(|(n, _, _)| n == "dexed"));
        assert!(preview.to_install.iter().any(|(n, _, _)| n == "helm"));

        // vital (same version)
        assert_eq!(preview.to_skip_same.len(), 1, "to_skip_same: expected 1");
        assert_eq!(preview.to_skip_same[0], "vital");

        // surge-xt (installed 1.3.1 > import 1.2.0)
        assert_eq!(preview.to_skip_newer.len(), 1, "to_skip_newer: expected 1");
        assert_eq!(preview.to_skip_newer[0].0, "surge-xt");

        // vital needs pinning (import pinned=true, installed pinned=false)
        assert_eq!(preview.to_pin.len(), 1, "to_pin: expected 1");
        assert_eq!(preview.to_pin[0], "vital");

        // community source (not in config) -> to_add
        assert_eq!(
            preview.to_add_sources.len(),
            1,
            "to_add_sources: expected 1"
        );
        assert_eq!(preview.to_add_sources[0].0, "community");

        // existing-diff source (URL mismatch)
        assert_eq!(
            preview.source_url_mismatches.len(),
            1,
            "source_url_mismatches: expected 1"
        );
        assert_eq!(preview.source_url_mismatches[0].0, "existing-diff");

        // Config change: user -> system
        assert!(
            !preview.config_changes.is_empty(),
            "config_changes should not be empty"
        );
        assert!(preview
            .config_changes
            .iter()
            .any(|c| c.contains("install_scope")));
    }

    #[test]
    fn test_encode_decode_with_sources() {
        let setup = PortableSetup {
            v: 1,
            p: vec![PortablePlugin {
                n: "vital".to_string(),
                v: "1.5.5".to_string(),
                p: false,
                s: "official".to_string(),
            }],
            s: vec![
                (
                    "community".to_string(),
                    "https://github.com/example/community-registry".to_string(),
                ),
                (
                    "private-lab".to_string(),
                    "https://git.internal.example.com/audio/plugins".to_string(),
                ),
            ],
            c: PortableConfig::default(),
        };

        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();

        assert_eq!(decoded.s.len(), 2, "expected 2 sources after round-trip");
        assert_eq!(decoded.s[0].0, "community");
        assert_eq!(
            decoded.s[0].1,
            "https://github.com/example/community-registry"
        );
        assert_eq!(decoded.s[1].0, "private-lab");
        assert_eq!(
            decoded.s[1].1,
            "https://git.internal.example.com/audio/plugins"
        );
    }

    #[test]
    fn test_encode_decode_with_pins() {
        let setup = PortableSetup {
            v: 1,
            p: vec![
                PortablePlugin {
                    n: "vital".to_string(),
                    v: "1.5.5".to_string(),
                    p: true,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "surge-xt".to_string(),
                    v: "1.3.1".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "dexed".to_string(),
                    v: "0.9.7".to_string(),
                    p: true,
                    s: "community".to_string(),
                },
            ],
            s: vec![],
            c: PortableConfig::default(),
        };

        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();

        assert_eq!(decoded.p.len(), 3, "expected 3 plugins after round-trip");
        assert!(decoded.p[0].p, "vital should be pinned");
        assert!(!decoded.p[1].p, "surge-xt should not be pinned");
        assert!(decoded.p[2].p, "dexed should be pinned");

        // Also verify that names survived the round-trip
        assert_eq!(decoded.p[0].n, "vital");
        assert_eq!(decoded.p[1].n, "surge-xt");
        assert_eq!(decoded.p[2].n, "dexed");
    }

    #[test]
    fn test_encode_decode_with_preferences() {
        let setup = PortableSetup {
            v: 1,
            p: vec![],
            s: vec![],
            c: PortableConfig {
                sc: Some("system".to_string()),
                reg: Some("https://private-registry.example.com/audio".to_string()),
            },
        };

        let encoded = encode(&setup).unwrap();
        let decoded = decode(&encoded).unwrap();

        assert_eq!(
            decoded.c.sc,
            Some("system".to_string()),
            "install_scope should be 'system' after round-trip"
        );
        assert_eq!(
            decoded.c.reg,
            Some("https://private-registry.example.com/audio".to_string()),
            "registry_url should survive round-trip"
        );
    }

    #[test]
    fn test_decode_rejects_invalid_prefix() {
        // "apm2://" is a different (non-existent) version prefix
        let result = decode("apm2://abc");
        assert!(
            result.is_err(),
            "apm2:// should be rejected as invalid prefix"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("apm1://"),
            "Error should mention the expected 'apm1://' prefix, got: {err}"
        );
    }

    #[test]
    fn test_decode_rejects_garbage() {
        // Valid prefix but the payload is not valid base64url-encoded deflated JSON
        let result = decode("apm1://this-is-not-valid!!base64@@data");
        assert!(
            result.is_err(),
            "Garbage payload after apm1:// should fail gracefully"
        );
        // Should not panic — any error type is acceptable
    }

    #[test]
    fn test_encode_produces_reasonably_short_string() {
        let slugs = [
            "vital",
            "surge-xt",
            "dexed",
            "helm",
            "odin2",
            "diva",
            "serum",
            "massive-x",
            "pigments",
            "phaseplant",
            "spire",
            "sylenth1",
            "analog-lab",
            "omnisphere",
            "kontakt",
            "zebra2",
            "repro",
            "hive2",
            "fm8",
            "reaktor",
        ];
        assert_eq!(slugs.len(), 20, "sanity: should have 20 plugin slugs");

        let plugins: Vec<PortablePlugin> = slugs
            .iter()
            .map(|s| PortablePlugin {
                n: s.to_string(),
                v: "1.0.0".to_string(),
                p: false,
                s: "official".to_string(),
            })
            .collect();

        let setup = PortableSetup {
            v: 1,
            p: plugins,
            s: vec![],
            c: PortableConfig::default(),
        };

        let encoded = encode(&setup).unwrap();
        assert!(
            encoded.len() < 1500,
            "Encoded string for 20 plugins should be under 1500 chars, got {} chars",
            encoded.len()
        );
    }

    #[test]
    fn test_build_preview_new_outdated_current() {
        // Three distinct categories:
        // - "synth-new" 2.0.0: not installed at all -> to_install (new)
        // - "synth-outdated" 3.0.0: installed at 2.0.0 (older) -> to_install (outdated)
        // - "synth-current" 1.0.0: installed at 1.0.0 (same) -> to_skip_same (current)
        let setup = PortableSetup {
            v: 1,
            p: vec![
                PortablePlugin {
                    n: "synth-new".to_string(),
                    v: "2.0.0".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "synth-outdated".to_string(),
                    v: "3.0.0".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
                PortablePlugin {
                    n: "synth-current".to_string(),
                    v: "1.0.0".to_string(),
                    p: false,
                    s: "official".to_string(),
                },
            ],
            s: vec![],
            c: PortableConfig::default(),
        };

        let state = InstallState {
            version: 1,
            plugins: vec![
                InstalledPlugin {
                    name: "synth-outdated".to_string(),
                    version: "2.0.0".to_string(),
                    vendor: "Test Vendor".to_string(),
                    formats: vec![],
                    installed_at: Utc::now(),
                    source: "official".to_string(),
                    pinned: false,
                },
                InstalledPlugin {
                    name: "synth-current".to_string(),
                    version: "1.0.0".to_string(),
                    vendor: "Test Vendor".to_string(),
                    formats: vec![],
                    installed_at: Utc::now(),
                    source: "official".to_string(),
                    pinned: false,
                },
            ],
        };

        let config = Config {
            default_registry_url: "https://github.com/apm-pm/registry".to_string(),
            install_scope: InstallScope::User,
            data_dir: None,
            cache_dir: None,
            sources: vec![],
        };

        let preview = build_preview(&setup, &state, &config);

        // "synth-new" (not installed) and "synth-outdated" (import 3.0.0 > installed 2.0.0)
        assert_eq!(
            preview.to_install.len(),
            2,
            "to_install: expected new + outdated = 2"
        );
        assert!(
            preview.to_install.iter().any(|(n, v, _)| n == "synth-new" && v == "2.0.0"),
            "synth-new should be in to_install"
        );
        assert!(
            preview
                .to_install
                .iter()
                .any(|(n, v, _)| n == "synth-outdated" && v == "3.0.0"),
            "synth-outdated should be in to_install"
        );

        // "synth-current" (same version)
        assert_eq!(
            preview.to_skip_same.len(),
            1,
            "to_skip_same: expected current = 1"
        );
        assert_eq!(preview.to_skip_same[0], "synth-current");

        // Nothing should be in skip_newer, pin, or unpin
        assert!(
            preview.to_skip_newer.is_empty(),
            "to_skip_newer should be empty"
        );
        assert!(preview.to_pin.is_empty(), "to_pin should be empty");
        assert!(preview.to_unpin.is_empty(), "to_unpin should be empty");
    }
}
