// Plugin matcher — connects scanned on-disk plugins to registry entries.
//
// Matching strategy (in priority order):
// 1. Bundle ID prefix match — most reliable, unique per product
// 2. Normalized name + vendor match — handles naming differences
// 3. Normalized name only — last resort, may produce false positives

use crate::bundle_id_store::BundleIdStore;
use crate::registry::{PluginDefinition, Registry};
use crate::scanner::ScannedPlugin;

/// The method by which a scanned plugin was matched to a registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMethod {
    /// Matched via `CFBundleIdentifier` prefix against `bundle_ids` in the registry.
    BundleId,
    /// Matched via normalized name + vendor comparison.
    NameAndVendor,
    /// Matched via normalized name only (weaker signal).
    NameOnly,
}

/// A scanned plugin matched to a registry entry.
#[derive(Debug, Clone)]
pub struct PluginMatch<'a> {
    pub registry_plugin: &'a PluginDefinition,
    pub method: MatchMethod,
}

/// Attempt to match a scanned plugin against the registry.
///
/// Returns the best match found, or `None` if no match is possible.
pub fn match_plugin<'a>(
    scanned: &ScannedPlugin,
    registry: &'a Registry,
    local_store: Option<&BundleIdStore>,
) -> Option<PluginMatch<'a>> {
    // Strategy 1a: Bundle ID prefix match from registry
    if !scanned.bundle_id.is_empty() {
        if let Some(plugin) = match_by_bundle_id(&scanned.bundle_id, registry) {
            return Some(PluginMatch {
                registry_plugin: plugin,
                method: MatchMethod::BundleId,
            });
        }

        // Strategy 1b: Bundle ID from local learned store
        if let Some(store) = local_store {
            if let Some(slug) = store.find_slug(&scanned.bundle_id) {
                if let Some(plugin) = registry.find(slug) {
                    return Some(PluginMatch {
                        registry_plugin: plugin,
                        method: MatchMethod::BundleId,
                    });
                }
            }
        }
    }

    // Strategy 2: Normalized name + vendor
    if !scanned.vendor.is_empty() {
        if let Some(plugin) = match_by_name_and_vendor(&scanned.name, &scanned.vendor, registry) {
            return Some(PluginMatch {
                registry_plugin: plugin,
                method: MatchMethod::NameAndVendor,
            });
        }
    }

    // Strategy 3: Normalized name only (full scanned name)
    if let Some(plugin) = match_by_name(&scanned.name, registry) {
        return Some(PluginMatch {
            registry_plugin: plugin,
            method: MatchMethod::NameOnly,
        });
    }

    // Strategy 4: Strip vendor prefix from scanned name and try again.
    // Many macOS plugins embed the vendor in CFBundleName (e.g., "FabFilter Pro-Q 4").
    if !scanned.vendor.is_empty() {
        let name_lower = scanned.name.to_lowercase();
        let vendor_lower = scanned.vendor.to_lowercase();
        if let Some(rest) = name_lower.strip_prefix(&vendor_lower) {
            let stripped = rest.trim_start_matches(|c: char| c == ' ' || c == '-' || c == ':');
            if !stripped.is_empty() {
                if let Some(plugin) = match_by_name(stripped, registry) {
                    return Some(PluginMatch {
                        registry_plugin: plugin,
                        method: MatchMethod::NameOnly,
                    });
                }
            }
        }
    }

    None
}

/// Match by comparing the scanned bundle ID against each registry entry's
/// `bundle_ids` list. Uses prefix matching to handle format/version suffixes
/// (e.g., `com.fabfilter.Pro-Q` matches `com.fabfilter.Pro-Q.AU.4`).
fn match_by_bundle_id<'a>(bundle_id: &str, registry: &'a Registry) -> Option<&'a PluginDefinition> {
    let bid_lower = bundle_id.to_lowercase();
    registry.all().into_iter().find(|plugin| {
        plugin
            .bundle_ids
            .iter()
            .any(|pattern| bid_lower.starts_with(&pattern.to_lowercase()))
    })
}

/// Match by comparing normalized names and vendors.
fn match_by_name_and_vendor<'a>(
    scanned_name: &str,
    scanned_vendor: &str,
    registry: &'a Registry,
) -> Option<&'a PluginDefinition> {
    let norm_name = normalize(scanned_name);
    let norm_vendor = normalize(scanned_vendor);

    registry.all().into_iter().find(|plugin| {
        normalize(&plugin.name) == norm_name && normalize(&plugin.vendor) == norm_vendor
    })
}

/// Match by comparing normalized names only. To reduce false positives,
/// requires an exact normalized match (not substring).
fn match_by_name<'a>(scanned_name: &str, registry: &'a Registry) -> Option<&'a PluginDefinition> {
    let norm = normalize(scanned_name);
    if norm.len() < 3 {
        return None; // Too short, too many false positives
    }
    registry
        .all()
        .into_iter()
        .find(|plugin| normalize(&plugin.name) == norm)
}

/// Normalize a name for comparison: lowercase, strip version suffixes,
/// remove punctuation, collapse whitespace.
fn normalize(s: &str) -> String {
    let mut n = s.to_lowercase();
    // Remove common format/version suffixes
    for suffix in [" au", " vst3", " vst", " aax", " component", " .vst3"] {
        n = n.trim_end_matches(suffix).to_string();
    }
    // Remove trailing version numbers like " 2", " v3", " 4.0"
    n = strip_trailing_version(&n);
    // Remove ALL non-alphanumeric chars (hyphens, spaces, punctuation all become nothing)
    n.retain(|c| c.is_alphanumeric());
    n
}

/// Strip trailing version-like patterns: " 2", " v3.1", " V4"
fn strip_trailing_version(s: &str) -> String {
    let s = s.trim();
    // Match patterns like " 2", " v3", " V4.1.2", " 3.0"
    if let Some(last_space) = s.rfind(' ') {
        let suffix = &s[last_space + 1..];
        let suffix_stripped = suffix.strip_prefix('v').or(suffix.strip_prefix('V')).unwrap_or(suffix);
        if suffix_stripped.chars().next().map_or(false, |c| c.is_ascii_digit())
            && suffix_stripped.chars().all(|c| c.is_ascii_digit() || c == '.')
        {
            return s[..last_space].to_string();
        }
    }
    s.to_string()
}

/// Extract a stable bundle ID prefix by stripping format and version suffixes.
///
/// e.g., `"com.fabfilter.Pro-Q.AU.4"` → `"com.fabfilter.Pro-Q"`
///       `"com.soundtoys.audiounit.EchoBoy"` → `"com.soundtoys.audiounit.EchoBoy"`
pub fn extract_bundle_id_prefix(bundle_id: &str) -> String {
    let mut bid = bundle_id.to_string();
    // Remove trailing format+version like .AU.4, .Vst3.2, .MusicDevice.component
    let format_patterns = [
        ".AU.", ".Vst3.", ".VST3.", ".AAX.", ".MusicDevice.", ".MusicEffect.", ".audiounit.",
    ];
    for pat in &format_patterns {
        if let Some(pos) = bid.find(pat) {
            bid.truncate(pos);
            return bid;
        }
    }
    // Remove trailing .component, .vst3
    for suffix in [".component", ".vst3"] {
        if bid.ends_with(suffix) {
            bid.truncate(bid.len() - suffix.len());
            return bid;
        }
    }
    bid
}

/// Auto-learn: given a scanned plugin that was matched by name/vendor,
/// record its bundle ID prefix in the local store for future fast matching.
pub fn auto_learn(
    scanned: &ScannedPlugin,
    matched_slug: &str,
    store: &mut BundleIdStore,
) -> bool {
    if scanned.bundle_id.is_empty() {
        return false;
    }
    let prefix = extract_bundle_id_prefix(&scanned.bundle_id);
    store.learn(&prefix, matched_slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_version_suffix() {
        assert_eq!(normalize("Pro-Q 4"), normalize("Pro-Q 3"));
        assert_eq!(normalize("Pro-Q 4"), normalize("Pro-Q"));
        assert_eq!(normalize("FabFilter Pro-L 2"), normalize("FabFilter Pro-L"));
    }

    #[test]
    fn normalize_strips_format_suffix() {
        assert_eq!(normalize("Crystallizer AU"), normalize("Crystallizer"));
        assert_eq!(normalize("Diva VST3"), normalize("Diva"));
    }

    #[test]
    fn normalize_case_insensitive() {
        assert_eq!(normalize("EchoBoy"), normalize("echoboy"));
    }

    #[test]
    fn normalize_handles_punctuation() {
        assert_eq!(normalize("Pro-Q"), normalize("ProQ"));
        assert_eq!(normalize("Pro-C 2"), normalize("Pro-C"));
    }

    #[test]
    fn strip_trailing_version_works() {
        assert_eq!(strip_trailing_version("Pro-Q 4"), "Pro-Q");
        assert_eq!(strip_trailing_version("Pro-L v2"), "Pro-L");
        assert_eq!(strip_trailing_version("Saturn 2.11"), "Saturn");
        assert_eq!(strip_trailing_version("EchoBoy"), "EchoBoy");
    }
}
