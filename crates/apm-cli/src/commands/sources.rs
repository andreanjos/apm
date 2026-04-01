use anyhow::Result;

use apm_core::config::{Config, SourceEntry};

pub async fn run_add(config: &Config, url: &str, name: Option<&str>) -> Result<()> {
    // Derive a name from the URL if not provided.
    let source_name = match name {
        Some(n) => n.to_string(),
        None => derive_name_from_url(url),
    };

    // Validate: don't allow "official" — that's the built-in default.
    if source_name == "official" {
        anyhow::bail!(
            "The name 'official' is reserved for the built-in default registry.\n\
             Hint: Choose a different name with `--name <name>`."
        );
    }

    // Check for duplicates.
    if config.sources.iter().any(|s| s.name == source_name) {
        anyhow::bail!(
            "A source named '{}' already exists.\n\
             Hint: Remove it first with `apm sources remove {}`, or choose a different name.",
            source_name,
            source_name
        );
    }
    if config.sources.iter().any(|s| s.url == url) {
        anyhow::bail!(
            "A source with URL '{}' already exists.\n\
             Hint: Run `apm sources list` to see configured sources.",
            url
        );
    }

    let mut updated = config.clone();
    updated.sources.push(SourceEntry {
        name: source_name.clone(),
        url: url.to_string(),
    });
    updated.save()?;

    println!("Added registry source '{source_name}' ({url}).");
    println!("Run `apm sync` to download plugins from this registry.");
    Ok(())
}

pub async fn run_remove(config: &Config, name: &str) -> Result<()> {
    if name == "official" {
        anyhow::bail!(
            "Cannot remove the default registry source 'official'.\n\
             Hint: To change the default registry URL, edit \
             ~/.config/apm/config.toml and update `default_registry_url`."
        );
    }

    let original_len = config.sources.len();
    let mut updated = config.clone();
    updated.sources.retain(|s| s.name != name);

    if updated.sources.len() == original_len {
        anyhow::bail!(
            "No source named '{}' found.\n\
             Hint: Run `apm sources list` to see all configured sources.",
            name
        );
    }

    updated.save()?;
    println!("Removed registry source '{name}'.");
    Ok(())
}

pub async fn run_list(config: &Config) -> Result<()> {
    let sources = config.sources();

    if sources.is_empty() {
        println!("No registry sources configured.");
        return Ok(());
    }

    // Column headers.
    const HDR_NAME: &str = "Name";
    const HDR_URL: &str = "URL";
    const HDR_TYPE: &str = "Type";

    let w_name = sources
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(0)
        .max(HDR_NAME.len());

    let w_url = sources
        .iter()
        .map(|s| s.url.len())
        .max()
        .unwrap_or(0)
        .max(HDR_URL.len());

    println!(
        "{:<w_name$}  {:<w_url$}  {}",
        HDR_NAME, HDR_URL, HDR_TYPE
    );
    println!("{}", "\u{2500}".repeat(w_name + 2 + w_url + 2 + HDR_TYPE.len()));

    for source in &sources {
        let type_label = if source.is_default { "default" } else { "user" };
        println!(
            "{:<w_name$}  {:<w_url$}  {}",
            source.name, source.url, type_label
        );
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive a short source name from a URL.
///
/// `https://github.com/acme/my-registry` → `my-registry`
fn derive_name_from_url(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("registry")
        .trim_end_matches(".git")
        .to_string()
}
