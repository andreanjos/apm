// Registry search — case-insensitive full-text search across plugin metadata
// with optional category filtering and relevance ranking.

use crate::registry::{PluginDefinition, Registry};

/// Search `registry` for plugins matching `query`, optionally restricted to
/// `category` (matches category or subcategory, case-insensitive) and/or
/// `vendor` (matches the vendor field, case-insensitive).
///
/// Results are sorted by relevance:
/// 1. Exact slug or name match (case-insensitive).
/// 2. Vendor name contains the query.
/// 3. Category / subcategory contains the query.
/// 4. Description or tags contain the query.
pub fn search<'r>(
    registry: &'r Registry,
    query: &str,
    category: Option<&str>,
    vendor: Option<&str>,
) -> Vec<&'r PluginDefinition> {
    let query_lower = query.to_lowercase();
    let category_lower = category.map(|c| c.to_lowercase());
    let vendor_lower = vendor.map(|v| v.to_lowercase());

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

            // If the query is empty (e.g. `apm search --category reverb ""`),
            // return all category/vendor-filtered results.
            if query_lower.is_empty() {
                return true;
            }

            text_matches(p, &query_lower)
        })
        .collect();

    // Sort by relevance tier, then alphabetically within a tier for stability.
    results.sort_by_key(|p| (relevance_score(p, &query_lower), p.name.to_lowercase()));

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
}

/// Lower score = higher relevance (used as sort key).
///
/// 0 — exact slug or name match
/// 1 — vendor contains query
/// 2 — category / subcategory contains query
/// 3 — description or tag contains query
fn relevance_score(p: &PluginDefinition, query: &str) -> u8 {
    let name_lower = p.name.to_lowercase();
    let slug_lower = p.slug.to_lowercase();

    if name_lower == query || slug_lower == query {
        return 0;
    }
    if name_lower.contains(query) || slug_lower.contains(query) {
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
