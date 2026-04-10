// import command — import a plugin setup from a portable apm1:// string or
// a legacy TOML/JSON export file. Shows a preview before proceeding.

use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::commands::export_cmd::{ExportDocument, ExportedPlugin};
use crate::portable;
use apm_core::config::{Config, SourceEntry};
use apm_core::registry::Registry;
use apm_core::state::InstallState;

// ── Input Detection ──────────────────────────────────────────────────────────

enum InputKind {
    PortableString(String),
    LegacyFile(std::path::PathBuf),
}

fn detect_input(input: &str) -> Result<InputKind> {
    if input.starts_with("apm1://") {
        return Ok(InputKind::PortableString(input.to_string()));
    }
    let path = Path::new(input);
    if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read file: {}", path.display()))?;
        let trimmed = content.trim();
        if trimmed.starts_with("apm1://") {
            return Ok(InputKind::PortableString(trimmed.to_string()));
        }
        return Ok(InputKind::LegacyFile(path.to_path_buf()));
    }
    anyhow::bail!(
        "Input is not a valid apm1:// string or an existing file: {input}\n\
         Hint: Portable strings start with 'apm1://'. For files, check the path exists."
    );
}

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn run(config: &Config, input: &str, dry_run: bool, yes: bool) -> Result<()> {
    match detect_input(input)? {
        InputKind::PortableString(s) => run_portable(config, &s, dry_run, yes).await,
        InputKind::LegacyFile(path) => run_legacy(config, &path, dry_run).await,
    }
}

// ── Portable import path (apm1://) ──────────────────────────────────────────

async fn run_portable(config: &Config, input: &str, dry_run: bool, yes: bool) -> Result<()> {
    let setup = portable::decode(input)?;
    let state = InstallState::load(config)?;
    let preview = portable::build_preview(&setup, &state, config);

    // Display preview
    println!("Preview:");
    for (slug, ver, pinned) in &preview.to_install {
        let pin_tag = if *pinned { " (pin)" } else { "" };
        println!("  {}   {} v{}{}", "install".cyan(), slug, ver, pin_tag);
    }
    for slug in &preview.to_skip_same {
        println!("  {}      {} (already installed)", "skip".dimmed(), slug);
    }
    for (slug, import_ver, installed_ver) in &preview.to_skip_newer {
        println!(
            "  {}      {} v{} (newer v{} installed)",
            "skip".dimmed(),
            slug,
            import_ver,
            installed_ver
        );
    }
    for slug in &preview.to_pin {
        println!("  {}       {} (will pin)", "pin".yellow(), slug);
    }
    for slug in &preview.to_unpin {
        println!("  {}     {} (will unpin)", "unpin".yellow(), slug);
    }
    for (name, url) in &preview.to_add_sources {
        println!("  {} source \"{}\" ({})", "add".cyan(), name, url);
    }
    for (name, import_url, existing_url) in &preview.source_url_mismatches {
        println!(
            "  {}  source \"{}\" URL differs: imported={}, existing={}",
            "warn".yellow().bold(),
            name,
            import_url,
            existing_url
        );
    }
    for change in &preview.config_changes {
        println!("  {}    {}", "config".blue(), change);
    }

    // Summary line
    let n_install = preview.to_install.len();
    let n_skip = preview.to_skip_same.len() + preview.to_skip_newer.len();
    let n_sources = preview.to_add_sources.len();
    let n_pin = preview.to_pin.len();
    println!();
    println!(
        "Would install {} plugins, skip {}, add {} sources, pin {}.",
        n_install, n_skip, n_sources, n_pin
    );

    // Nothing to do?
    if n_install == 0
        && n_sources == 0
        && preview.to_pin.is_empty()
        && preview.to_unpin.is_empty()
        && preview.config_changes.is_empty()
    {
        println!("Nothing to do -- current setup matches.");
        return Ok(());
    }

    // Dry-run exit
    if dry_run {
        println!("{}", "(dry-run mode -- no changes will be made)".dimmed());
        return Ok(());
    }

    // Confirmation prompt
    if !yes {
        print!("Proceed? [Y/n] ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input_buf = String::new();
        std::io::stdin()
            .read_line(&mut input_buf)
            .context("Cannot read user input")?;
        if !input_buf.trim().eq_ignore_ascii_case("y") && !input_buf.trim().is_empty() {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Execute changes
    let registry = Registry::load_all_sources(config)?;
    let mut state = InstallState::load(config)?;

    let mut installed = 0usize;
    let skipped = preview.to_skip_same.len() + preview.to_skip_newer.len();
    let mut failed = 0usize;

    for (slug, version, _pinned) in &preview.to_install {
        // Look up in registry — prefer the source from the portable setup
        let source_name = setup
            .p
            .iter()
            .find(|p| p.n == *slug)
            .map(|p| p.s.as_str())
            .unwrap_or("official");

        let plugin = match registry
            .find_in_source(source_name, slug)
            .or_else(|| registry.find(slug))
        {
            Some(p) => p,
            None => {
                eprintln!(
                    "  {} {}: not found in registry (try `apm sync`)",
                    "FAILED".red().bold(),
                    slug
                );
                failed += 1;
                continue;
            }
        };

        let release = match plugin.resolve_release(Some(version)) {
            Some(r) => r,
            None => {
                let available = plugin.available_versions().join(", ");
                eprintln!(
                    "  {} {}: version {} not found (available: {})",
                    "FAILED".red().bold(),
                    slug,
                    version,
                    available
                );
                failed += 1;
                continue;
            }
        };

        let mut selected = plugin.clone();
        selected.version = release.version;
        selected.formats = release.formats;

        println!(
            "  {} {} v{}...",
            "installing".cyan(),
            slug,
            selected.version
        );

        match crate::install::install_plugin(&selected, None, None, config, &mut state, None).await
        {
            Ok(()) => {
                println!("  {} {} v{}", "installed".green(), slug, selected.version);
                installed += 1;
            }
            Err(e) => {
                eprintln!("  {} {}: {}", "FAILED".red().bold(), slug, e);
                failed += 1;
            }
        }
    }

    // Add sources
    let mut sources_added = 0usize;
    if !preview.to_add_sources.is_empty() {
        let mut config_mut = config.clone();
        for (name, url) in &preview.to_add_sources {
            config_mut.sources.push(SourceEntry {
                name: name.clone(),
                url: url.clone(),
            });
            println!("  Added source \"{}\"", name);
            sources_added += 1;
        }
        config_mut.save()?;
    }

    // Pin/unpin changes
    let mut pin_changes = 0usize;
    for slug in &preview.to_pin {
        if let Some(p) = state.find_mut(slug) {
            p.pinned = true;
            pin_changes += 1;
        }
    }
    for slug in &preview.to_unpin {
        if let Some(p) = state.find_mut(slug) {
            p.pinned = false;
            pin_changes += 1;
        }
    }
    if pin_changes > 0 {
        state.save(config)?;
    }

    // Final summary
    let summary = format!(
        "Imported: {} installed, {} skipped, {} failed, {} sources added, {} pin changes.",
        installed, skipped, failed, sources_added, pin_changes
    );

    if failed == 0 {
        println!("\n{}", summary.green());
    } else {
        println!("\n{}", summary.yellow());
    }

    Ok(())
}

// ── Legacy import path (TOML / JSON files) ──────────────────────────────────

async fn run_legacy(config: &Config, file: &Path, dry_run: bool) -> Result<()> {
    let doc = load_export_file(file)?;

    if doc.plugins.is_empty() {
        println!("No plugins found in {}.", file.display());
        return Ok(());
    }

    println!(
        "Found {} plugin(s) in {}.",
        doc.plugins.len(),
        file.display()
    );

    if dry_run {
        println!("{}", "(dry-run mode -- no changes will be made)".dimmed());
    }

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    let mut state = InstallState::load(config)?;

    let mut installed = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    for entry in &doc.plugins {
        match process_one(config, entry, &registry, &mut state, dry_run).await {
            PluginOutcome::Installed => {
                installed += 1;
            }
            PluginOutcome::Skipped => {
                skipped += 1;
            }
            PluginOutcome::Failed(reason) => {
                eprintln!("  {} {}: {}", "FAILED".red().bold(), entry.name, reason);
                failed += 1;
            }
        }
    }

    let total = doc.plugins.len();
    let suffix = if dry_run { " (dry-run)" } else { "" };
    let summary = format!(
        "Imported {total} plugins ({installed} installed, {skipped} skipped, {failed} failed){suffix}"
    );

    if failed == 0 {
        println!("\n{}", summary.green());
    } else {
        println!("\n{}", summary.yellow());
    }

    Ok(())
}

// ── Per-plugin logic (legacy path) ──────────────────────────────────────────

enum PluginOutcome {
    Installed,
    Skipped,
    Failed(String),
}

async fn process_one(
    config: &Config,
    entry: &ExportedPlugin,
    registry: &Registry,
    state: &mut InstallState,
    dry_run: bool,
) -> PluginOutcome {
    if let Some(installed) = state.find(&entry.name) {
        if installed.version == entry.version {
            println!(
                "  {} {} v{} (already installed)",
                "skip".dimmed(),
                entry.name,
                entry.version
            );
            return PluginOutcome::Skipped;
        }
    }

    // Look up in registry.
    let plugin = match registry
        .find_in_source(&entry.source, &entry.name)
        .or_else(|| registry.find(&entry.name))
    {
        Some(p) => p,
        None => {
            return PluginOutcome::Failed("not found in registry (try `apm sync`)".to_string());
        }
    };

    let release = match plugin.resolve_release(Some(&entry.version)) {
        Some(release) => release,
        None => {
            let available = plugin.available_versions().join(", ");
            return PluginOutcome::Failed(format!(
                "version {} not found in registry (available: {})",
                entry.version, available
            ));
        }
    };

    let mut selected_plugin = plugin.clone();
    selected_plugin.version = release.version;
    selected_plugin.formats = release.formats;

    if dry_run {
        println!(
            "  {} {} v{}",
            "would install".cyan(),
            entry.name,
            selected_plugin.version
        );
        return PluginOutcome::Installed; // count as "would install"
    }

    println!(
        "  {} {} v{}...",
        "installing".cyan(),
        entry.name,
        selected_plugin.version
    );

    match crate::install::install_plugin(&selected_plugin, None, None, config, state, None).await {
        Ok(()) => {
            println!(
                "  {} {} v{}",
                "installed".green(),
                entry.name,
                selected_plugin.version
            );
            PluginOutcome::Installed
        }
        Err(e) => PluginOutcome::Failed(e.to_string()),
    }
}

// ── File loading (legacy path) ──────────────────────────────────────────────

/// Load and parse an export file, auto-detecting format by extension.
fn load_export_file(path: &Path) -> Result<ExportDocument> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read import file: {}", path.display()))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "json" {
        return serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse JSON import file: {}", path.display()));
    }

    // Try TOML first (covers .toml and unknown extensions).
    if let Ok(doc) = toml::from_str::<ExportDocument>(&raw) {
        return Ok(doc);
    }

    // Fall back to JSON.
    serde_json::from_str(&raw).with_context(|| {
        format!(
            "Failed to parse import file as TOML or JSON: {}",
            path.display()
        )
    })
}
