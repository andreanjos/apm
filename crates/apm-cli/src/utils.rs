//! Shared display helpers used across CLI commands.

use std::path::Path;

/// Replace the user's home directory prefix with `~` for readability.
pub fn display_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.to_string_lossy();
        if let Some(rest) = path_str.strip_prefix(home_str.as_ref()) {
            return format!("~{rest}");
        }
    }
    path_str.into_owned()
}

/// Truncate `s` to `max` characters, appending "..." if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        // Ensure the suffix fits: we always have max >= 3 from our constants.
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

/// Format a price for display.
///
/// Returns "free" when `is_paid` is false, a formatted currency string when
/// both `price_cents` and `currency` are present, or "paid" as a fallback.
pub fn format_price(price_cents: Option<i64>, currency: Option<&str>, is_paid: bool) -> String {
    if !is_paid {
        return "free".to_string();
    }

    match (price_cents, currency) {
        (Some(cents), Some(currency)) => {
            let major = cents / 100;
            let minor = cents.abs() % 100;
            format!("{} {}.{minor:02}", currency.to_uppercase(), major)
        }
        _ => "paid".to_string(),
    }
}

/// Format a category with an optional subcategory as "category / subcategory".
///
/// An empty subcategory string is treated the same as `None`.
pub fn format_category(category: &str, subcategory: Option<&str>) -> String {
    match subcategory {
        Some(sub) if !sub.is_empty() => format!("{category} / {sub}"),
        _ => category.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ── display_path ─────────────────────────────────────────────────────────

    #[test]
    fn display_path_abbreviates_home_prefix() {
        let home = dirs::home_dir().expect("home dir should be available in test");
        let under_home = home.join("Library/Audio/Plug-Ins/test.vst3");
        let result = display_path(&under_home);
        assert!(
            result.starts_with("~/"),
            "expected path under home to start with ~/, got: {result}"
        );
        assert!(
            result.contains("Library/Audio/Plug-Ins/test.vst3"),
            "expected rest of path preserved, got: {result}"
        );
    }

    #[test]
    fn display_path_not_under_home_stays_as_is() {
        let path = PathBuf::from("/tmp/some/random/path");
        let result = display_path(&path);
        assert_eq!(result, "/tmp/some/random/path");
    }

    #[test]
    fn display_path_root_stays_as_is() {
        let path = PathBuf::from("/");
        let result = display_path(&path);
        assert_eq!(result, "/");
    }

    // ── truncate ─────────────────────────────────────────────────────────────

    #[test]
    fn truncate_shorter_than_max_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_equal_to_max_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_longer_than_max_adds_ellipsis() {
        let result = truncate("hello world", 8);
        // max=8, saturating_sub(3)=5, so first 5 chars + "..."
        assert_eq!(result, "hello...");
    }

    #[test]
    fn truncate_very_short_max() {
        // max=3: saturating_sub(3)=0, so we get just "..."
        let result = truncate("hello", 3);
        assert_eq!(result, "...");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 10), "");
    }

    // ── format_price ─────────────────────────────────────────────────────────

    #[test]
    fn format_price_free() {
        assert_eq!(format_price(None, None, false), "free");
    }

    #[test]
    fn format_price_free_ignores_cents_when_not_paid() {
        assert_eq!(format_price(Some(999), Some("USD"), false), "free");
    }

    #[test]
    fn format_price_paid_with_cents_usd() {
        let result = format_price(Some(999), Some("usd"), true);
        assert_eq!(result, "USD 9.99");
    }

    #[test]
    fn format_price_paid_with_cents_eur() {
        let result = format_price(Some(1499), Some("eur"), true);
        assert_eq!(result, "EUR 14.99");
    }

    #[test]
    fn format_price_paid_whole_dollar() {
        let result = format_price(Some(5000), Some("usd"), true);
        assert_eq!(result, "USD 50.00");
    }

    #[test]
    fn format_price_paid_no_cents_fallback() {
        // is_paid=true but price_cents=None -> fallback "paid"
        assert_eq!(format_price(None, Some("usd"), true), "paid");
    }

    #[test]
    fn format_price_paid_no_currency_fallback() {
        // is_paid=true but currency=None -> fallback "paid"
        assert_eq!(format_price(Some(999), None, true), "paid");
    }

    #[test]
    fn format_price_paid_nothing_fallback() {
        // is_paid=true, no cents, no currency -> fallback "paid"
        assert_eq!(format_price(None, None, true), "paid");
    }

    // ── format_category ──────────────────────────────────────────────────────

    #[test]
    fn format_category_with_subcategory() {
        assert_eq!(
            format_category("effects", Some("reverb")),
            "effects / reverb"
        );
    }

    #[test]
    fn format_category_without_subcategory() {
        assert_eq!(format_category("effects", None), "effects");
    }

    #[test]
    fn format_category_empty_subcategory_treated_as_none() {
        assert_eq!(format_category("effects", Some("")), "effects");
    }
}
