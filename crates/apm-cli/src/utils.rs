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
pub fn format_category(category: &str, subcategory: Option<&str>) -> String {
    match subcategory {
        Some(sub) => format!("{category} / {sub}"),
        None => category.to_string(),
    }
}
