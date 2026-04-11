// Registry search — case-insensitive full-text search across plugin metadata
// with optional category filtering and relevance ranking.

use crate::registry::{PluginDefinition, ProductType, Registry};

/// Search `registry` for plugins matching `query`, optionally restricted to
/// `category` (matches category or subcategory, case-insensitive),
/// `vendor` (matches the vendor field, case-insensitive), and/or
/// `tag` (matches any element of the plugin's tags array, case-insensitive).
///
/// Results are sorted by relevance:
/// 1. Exact slug, name, or alias match (case-insensitive).
/// 2. Slug, name, or alias contains the query.
/// 3. Vendor name contains the query.
/// 4. Category / subcategory contains the query.
/// 5. Description or tags contain the query.
///
/// Standalone plugins are then preferred over bundles, upgrades, and other
/// non-plugin catalog items when the text match is otherwise tied.
pub fn search<'r>(
    registry: &'r Registry,
    query: &str,
    category: Option<&str>,
    vendor: Option<&str>,
    tag: Option<&str>,
) -> Vec<&'r PluginDefinition> {
    let query_lower = query.to_lowercase();
    let category_lower = category.map(|c| c.to_lowercase());
    let vendor_lower = vendor.map(|v| v.to_lowercase());
    let tag_lower = tag.map(|t| t.to_lowercase());

    // First pass: collect every plugin that matches the query (all fields).
    let mut results: Vec<&PluginDefinition> = registry
        .plugins
        .values()
        .filter(|p| {
            // Category filter applied first (fast path).
            if let Some(ref cat) = category_lower {
                let cat_match = p.category.to_lowercase().contains(cat.as_str())
                    || p.subcategory
                        .as_deref()
                        .map(|s| s.to_lowercase().contains(cat.as_str()))
                        .unwrap_or(false);
                if !cat_match {
                    return false;
                }
            }

            // Vendor filter.
            if let Some(ref ven) = vendor_lower {
                if !p.vendor.to_lowercase().contains(ven.as_str()) {
                    return false;
                }
            }

            // Tag filter — plugin must have a tag matching (case-insensitive).
            if let Some(ref tg) = tag_lower {
                let tag_match = p.tags.iter().any(|t| t.to_lowercase() == *tg);
                if !tag_match {
                    return false;
                }
            }

            // If the query is empty (e.g. `apm search --category reverb ""`),
            // return all category/vendor/tag-filtered results.
            if query_lower.is_empty() {
                return true;
            }

            text_matches(p, &query_lower)
        })
        .collect();

    // Sort by relevance tier, then prefer standalone plugins over catalog
    // items like bundles/upgrades when the textual match is otherwise similar.
    results.sort_by_key(|p| {
        (
            relevance_score(p, &query_lower),
            product_type_rank(&p.product_type),
            p.name.to_lowercase(),
        )
    });

    results
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns true when the plugin matches `query` in any searchable field.
fn text_matches(p: &PluginDefinition, query: &str) -> bool {
    p.slug.to_lowercase().contains(query)
        || p.name.to_lowercase().contains(query)
        || p.vendor.to_lowercase().contains(query)
        || p.description.to_lowercase().contains(query)
        || p.category.to_lowercase().contains(query)
        || p.subcategory
            .as_deref()
            .map(|s| s.to_lowercase().contains(query))
            .unwrap_or(false)
        || p.tags.iter().any(|t| t.to_lowercase().contains(query))
        || p.aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(query))
}

/// Lower score = higher relevance (used as sort key).
///
/// 0 — exact slug, name, or alias match
/// 1 — slug, name, or alias contains query
/// 2 — vendor contains query
/// 3 — category / subcategory contains query
/// 4 — description or tag contains query
fn relevance_score(p: &PluginDefinition, query: &str) -> u8 {
    let name_lower = p.name.to_lowercase();
    let slug_lower = p.slug.to_lowercase();

    if name_lower == query
        || slug_lower == query
        || p.aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(query))
    {
        return 0;
    }
    if name_lower.contains(query)
        || slug_lower.contains(query)
        || p.aliases
            .iter()
            .any(|alias| alias.to_lowercase().contains(query))
    {
        return 1;
    }
    if p.vendor.to_lowercase().contains(query) {
        return 2;
    }
    if p.category.to_lowercase().contains(query)
        || p.subcategory
            .as_deref()
            .map(|s| s.to_lowercase().contains(query))
            .unwrap_or(false)
    {
        return 3;
    }
    4
}

fn product_type_rank(product_type: &ProductType) -> u8 {
    match product_type {
        ProductType::Plugin => 0,
        ProductType::Daw => 1,
        ProductType::Utility => 1,
        ProductType::SampleLibrary => 2,
        ProductType::Expansion | ProductType::PresetPack => 3,
        ProductType::Bundle => 4,
        ProductType::Upgrade | ProductType::Subscription | ProductType::Template => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a minimal `PluginDefinition` with the given fields; everything
    /// else gets sensible defaults so tests stay concise.
    fn make_plugin(
        slug: &str,
        name: &str,
        vendor: &str,
        description: &str,
        category: &str,
        subcategory: Option<&str>,
        tags: Vec<&str>,
    ) -> PluginDefinition {
        PluginDefinition {
            slug: slug.to_string(),
            name: name.to_string(),
            vendor: vendor.to_string(),
            version: "1.0.0".to_string(),
            description: description.to_string(),
            category: category.to_string(),
            product_type: crate::registry::ProductType::Plugin,
            subcategory: subcategory.map(|s| s.to_string()),
            license: "Freeware".to_string(),
            tags: tags.into_iter().map(|t| t.to_string()).collect(),
            aliases: vec![],
            installer: None,
            formats: HashMap::new(),
            releases: Vec::new(),
            homepage: None,
            purchase_url: None,
            bundle_ids: vec![],
            is_paid: false,
            price_cents: None,
            currency: None,
            source_name: None,
        }
    }

    /// Build a `Registry` from a vec of `PluginDefinition`s.
    fn make_registry(plugins: Vec<PluginDefinition>) -> Registry {
        let mut map = HashMap::new();
        for p in plugins {
            map.insert(p.slug.clone(), p);
        }
        Registry {
            plugins: map,
            plugins_by_source: HashMap::new(),
            bundles: HashMap::new(),
            installers: HashMap::new(),
        }
    }

    #[test]
    fn test_exact_slug_match_ranks_first() {
        let registry = make_registry(vec![
            make_plugin(
                "surge-xt-effects",
                "Surge XT Effects",
                "Surge Synth Team",
                "Effect plugins from the Surge project",
                "effect",
                None,
                vec![],
            ),
            make_plugin(
                "surge-xt",
                "Surge XT",
                "Surge Synth Team",
                "Open-source hybrid synthesizer",
                "instrument",
                Some("synth"),
                vec![],
            ),
        ]);

        let results = search(&registry, "surge-xt", None, None, None);

        assert!(
            results.len() >= 2,
            "Expected at least 2 results, got {}",
            results.len()
        );
        assert_eq!(
            results[0].slug, "surge-xt",
            "Exact slug match should rank first"
        );
        assert_eq!(results[1].slug, "surge-xt-effects");
    }

    #[test]
    fn test_name_match_ranks_above_description() {
        let registry = make_registry(vec![
            make_plugin(
                "delay-machine",
                "Delay Machine",
                "Acme Audio",
                "Delay effect with a lush reverb tail",
                "effect",
                Some("delay"),
                vec![],
            ),
            make_plugin(
                "super-reverb",
                "Super Reverb",
                "Acme Audio",
                "A pristine algorithmic effect",
                "effect",
                Some("reverb"),
                vec![],
            ),
        ]);

        let results = search(&registry, "reverb", None, None, None);

        assert!(
            results.len() >= 2,
            "Expected at least 2 results, got {}",
            results.len()
        );
        assert_eq!(
            results[0].slug, "super-reverb",
            "Plugin with 'reverb' in name should rank above one with 'reverb' only in description"
        );
    }

    #[test]
    fn test_plugin_product_type_ranks_above_non_plugins() {
        let mut bundle = make_plugin(
            "synth-bundle",
            "Synth Bundle",
            "Acme Audio",
            "A suite containing multiple synth products",
            "bundle",
            None,
            vec![],
        );
        bundle.product_type = ProductType::Bundle;

        let registry = make_registry(vec![
            bundle,
            make_plugin(
                "super-synth",
                "Super Synth",
                "Acme Audio",
                "A standalone synth plugin",
                "instrument",
                Some("synth"),
                vec![],
            ),
        ]);

        let results = search(&registry, "synth", None, None, None);

        assert!(
            results.len() >= 2,
            "Expected at least 2 results, got {}",
            results.len()
        );
        assert_eq!(
            results[0].slug, "super-synth",
            "Standalone plugins should rank ahead of bundles when relevance is similar"
        );
    }

    #[test]
    fn test_vendor_filter_works() {
        let registry = make_registry(vec![
            make_plugin(
                "tal-noisemaker",
                "TAL-NoiseMaker",
                "TAL Software",
                "Virtual analog synthesizer",
                "instrument",
                Some("synth"),
                vec![],
            ),
            make_plugin(
                "vital",
                "Vital",
                "Matt Tytel",
                "Spectral warping wavetable synth",
                "instrument",
                Some("synth"),
                vec![],
            ),
            make_plugin(
                "tal-reverb",
                "TAL-Reverb",
                "TAL Software",
                "Plate reverb effect",
                "effect",
                Some("reverb"),
                vec![],
            ),
        ]);

        let results = search(&registry, "", None, Some("TAL Software"), None);

        assert_eq!(results.len(), 2, "Should return only TAL Software plugins");
        for p in &results {
            assert_eq!(
                p.vendor, "TAL Software",
                "All results should be from TAL Software"
            );
        }
    }

    #[test]
    fn test_category_filter_works() {
        let registry = make_registry(vec![
            make_plugin(
                "dexed",
                "Dexed",
                "Digital Suburban",
                "DX7 FM synthesizer",
                "instrument",
                Some("synth"),
                vec![],
            ),
            make_plugin(
                "dragonfly-reverb",
                "Dragonfly Reverb",
                "Michael Willis",
                "Algorithmic reverb",
                "effect",
                Some("reverb"),
                vec![],
            ),
            make_plugin(
                "odin2",
                "Odin 2",
                "The Wave Warden",
                "Semi-modular synthesizer",
                "instrument",
                Some("synth"),
                vec![],
            ),
        ]);

        let results = search(&registry, "", Some("instrument"), None, None);

        assert_eq!(results.len(), 2, "Should return only instruments");
        for p in &results {
            assert_eq!(
                p.category, "instrument",
                "All results should be instruments"
            );
        }
    }

    #[test]
    fn test_empty_query_returns_all() {
        let registry = make_registry(vec![
            make_plugin(
                "plugin-a",
                "Plugin A",
                "Vendor A",
                "First plugin",
                "effect",
                None,
                vec![],
            ),
            make_plugin(
                "plugin-b",
                "Plugin B",
                "Vendor B",
                "Second plugin",
                "instrument",
                None,
                vec![],
            ),
            make_plugin(
                "plugin-c",
                "Plugin C",
                "Vendor C",
                "Third plugin",
                "effect",
                Some("reverb"),
                vec![],
            ),
        ]);

        let results = search(&registry, "", None, None, None);

        assert_eq!(
            results.len(),
            3,
            "Empty query with no filters should return all plugins"
        );
    }

    #[test]
    fn test_case_insensitive_search() {
        let registry = make_registry(vec![
            make_plugin(
                "tal-noisemaker",
                "TAL-NoiseMaker",
                "TAL Software",
                "Virtual analog synthesizer",
                "instrument",
                Some("synth"),
                vec![],
            ),
            make_plugin(
                "vital",
                "Vital",
                "Matt Tytel",
                "Spectral warping wavetable synth",
                "instrument",
                Some("synth"),
                vec![],
            ),
        ]);

        let upper = search(&registry, "TAL", None, None, None);
        let lower = search(&registry, "tal", None, None, None);

        assert_eq!(
            upper.len(),
            lower.len(),
            "Case should not affect number of results"
        );
        assert!(
            !upper.is_empty(),
            "Should find at least one result for 'TAL'"
        );
        let upper_slugs: Vec<&str> = upper.iter().map(|p| p.slug.as_str()).collect();
        let lower_slugs: Vec<&str> = lower.iter().map(|p| p.slug.as_str()).collect();
        assert_eq!(
            upper_slugs, lower_slugs,
            "Results should be identical regardless of query case"
        );
    }

    #[test]
    fn test_tag_filter_works() {
        let registry = make_registry(vec![
            make_plugin(
                "surge-xt",
                "Surge XT",
                "Surge Synth Team",
                "Open-source hybrid synthesizer",
                "instrument",
                Some("synth"),
                vec!["open-source", "synth", "wavetable"],
            ),
            make_plugin(
                "vital",
                "Vital",
                "Matt Tytel",
                "Spectral warping wavetable synth",
                "instrument",
                Some("synth"),
                vec!["wavetable", "free"],
            ),
            make_plugin(
                "dragonfly-reverb",
                "Dragonfly Reverb",
                "Michael Willis",
                "Algorithmic reverb",
                "effect",
                Some("reverb"),
                vec!["open-source", "reverb"],
            ),
        ]);

        // Filter by "open-source" tag — should return Surge XT and Dragonfly.
        let results = search(&registry, "", None, None, Some("open-source"));
        assert_eq!(results.len(), 2, "Should return only open-source plugins");
        let slugs: Vec<&str> = results.iter().map(|p| p.slug.as_str()).collect();
        assert!(slugs.contains(&"surge-xt"));
        assert!(slugs.contains(&"dragonfly-reverb"));

        // Filter by "wavetable" tag — should return Surge XT and Vital.
        let results = search(&registry, "", None, None, Some("wavetable"));
        assert_eq!(results.len(), 2, "Should return only wavetable plugins");

        // Tag filter is case-insensitive.
        let results = search(&registry, "", None, None, Some("Open-Source"));
        assert_eq!(results.len(), 2, "Tag filter should be case-insensitive");

        // Tag filter combined with query.
        let results = search(&registry, "surge", None, None, Some("wavetable"));
        assert_eq!(results.len(), 1, "Should match query AND tag");
        assert_eq!(results[0].slug, "surge-xt");

        // Non-existent tag returns nothing.
        let results = search(&registry, "", None, None, Some("nonexistent"));
        assert!(results.is_empty(), "Unknown tag should match nothing");
    }
}
