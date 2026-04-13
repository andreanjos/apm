// install command — look up plugin(s) in registry, check install state,
// download, verify, extract, place, and record.
//
// Supports batch installs: `apm install vital surge-xt dexed`

use std::io::Read as _;
use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use apm_core::config::{Config, InstallScope};
use apm_core::error::ApmError;
use apm_core::registry::{
    DownloadType, FormatSource, InstallerDefinition, PluginDefinition, PluginFormat, Registry,
};
use apm_core::state::InstallState;

#[derive(Serialize)]
struct InstallFormatJson {
    format: String,
    download_type: String,
    source: String,
}

#[derive(Serialize)]
struct InstallPlanJson<'a> {
    plugin: &'a str,
    name: &'a str,
    version: &'a str,
    status: &'a str,
    destination: Option<&'a str>,
    formats: Vec<InstallFormatJson>,
    installer: Option<InstallerJson<'a>>,
    message: String,
}

#[derive(Serialize)]
struct InstallerJson<'a> {
    key: &'a str,
    name: &'a str,
    download_url: &'a str,
    homepage: &'a str,
    installed_app_path: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    config: &Config,
    plugins: &[String],
    stdin: bool,
    version: Option<&str>,
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
    from_file: Option<&Path>,
    dry_run: bool,
    bundle: Option<&str>,
    json: bool,
) -> Result<()> {
    // ── Resolve plugin list (--stdin or positional args) ─────────────────────

    let stdin_plugins;
    let plugins = if stdin {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        stdin_plugins = input
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        if stdin_plugins.is_empty() {
            anyhow::bail!(
                "No plugin names received from stdin.\n\
                 Hint: Pipe plugin names (space or newline separated) into `apm install --stdin`."
            );
        }
        stdin_plugins.as_slice()
    } else {
        plugins
    };

    // ── Validate --from-file with multiple plugins ────────────────────────────

    if from_file.is_some() && plugins.len() > 1 {
        anyhow::bail!(
            "--from-file can only be used when installing a single plugin.\n\
             Hint: Remove extra plugin names or omit --from-file."
        );
    }

    if bundle.is_some() && !plugins.is_empty() {
        anyhow::bail!(
            "--bundle cannot be combined with positional plugin names.\n\
             Hint: Use either `apm install --bundle <name>` or `apm install <plugin> ...`."
        );
    }

    if bundle.is_some() && stdin {
        anyhow::bail!(
            "--bundle cannot be combined with --stdin.\n\
             Hint: Use `apm install --bundle <name>` by itself for bundle installs."
        );
    }

    if bundle.is_some() && from_file.is_some() {
        anyhow::bail!(
            "--bundle cannot be combined with --from-file.\n\
             Hint: `--from-file` only applies to single-plugin archive installs."
        );
    }

    if version.is_some() && plugins.len() > 1 {
        anyhow::bail!(
            "--version can only be used when installing a single plugin.\n\
             Hint: Install one plugin at a time when selecting an explicit historical version."
        );
    }

    // ── Load registry ─────────────────────────────────────────────────────────

    let registry = Registry::load_all_sources(config)?;

    if registry.is_empty() {
        anyhow::bail!(
            "Registry cache is empty.\n\
             Hint: Run `apm sync` to populate the local registry cache."
        );
    }

    // ── Bundle resolution ─────────────────────────────────────────────────────

    if let Some(bundle_slug) = bundle {
        if json {
            anyhow::bail!(
                "`apm install --json --bundle` is not supported yet.\n\
                 Hint: Use `apm bundles --json` to inspect bundle contents, then install without --json."
            );
        }
        if version.is_some() {
            anyhow::bail!(
                "--version cannot be combined with --bundle.\n\
                 Hint: Bundle installs currently resolve each plugin to its latest registry version."
            );
        }

        let b = registry.find_bundle(bundle_slug).ok_or_else(|| {
            anyhow::anyhow!(
                "Bundle '{}' not found. Use `apm bundles` to list available bundles.",
                bundle_slug
            )
        })?;

        // Warn about plugins in the bundle that aren't in the registry.
        let missing: Vec<&String> = b
            .plugins
            .iter()
            .filter(|slug| registry.find(slug).is_none())
            .collect();

        if !missing.is_empty() {
            eprintln!(
                "{} {} plugin{} in this bundle {} not in the registry (will be skipped):",
                "⚠".yellow(),
                missing.len(),
                if missing.len() == 1 { "" } else { "s" },
                if missing.len() == 1 { "is" } else { "are" }
            );
            for slug in &missing {
                eprintln!("  - {slug}");
            }
            eprintln!();
        }

        println!(
            "Installing bundle '{}' ({} plugins)...",
            b.name,
            b.plugins.len()
        );

        let bundle_plugins: Vec<String> = b.plugins.clone();
        return Box::pin(run(
            config,
            &bundle_plugins,
            false,
            None,
            format,
            scope,
            None,
            dry_run,
            None,
            false,
        ))
        .await;
    }

    // ── Single-plugin fast path (original behaviour) ──────────────────────────

    if plugins.len() == 1 {
        let name = &plugins[0];
        return run_single(
            config, name, version, &registry, format, scope, from_file, dry_run, None, json,
        )
        .await;
    }

    // ── Batch install ─────────────────────────────────────────────────────────
    if json {
        anyhow::bail!(
            "`apm install --json` currently supports one plugin at a time.\n\
             Hint: Use `apm install --json <plugin> --dry-run` for machine-readable planning."
        );
    }

    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new(); // (name, reason)

    let total = plugins.len();
    for (i, name) in plugins.iter().enumerate() {
        let batch_prefix = format!("[{}/{}] ", i + 1, total);
        match run_single(
            config,
            name,
            None,
            &registry,
            format,
            scope,
            None,
            dry_run,
            Some(&batch_prefix),
            false,
        )
        .await
        {
            Ok(()) => {
                succeeded.push(name.clone());
            }
            Err(e) => {
                // Print the per-plugin failure immediately so the user can see it
                // as it happens (other plugins still continue).
                eprintln!("  {} {}: {}", "FAILED".red().bold(), name, e);
                failed.push((name.clone(), e.to_string()));
            }
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    if dry_run {
        return Ok(());
    }

    let n_ok = succeeded.len();
    let n_fail = failed.len();

    println!();
    if n_fail == 0 {
        println!(
            "{}",
            format!("Installed {n_ok}/{total} plugins successfully.").green()
        );
    } else {
        let failed_names: Vec<String> = failed
            .iter()
            .map(|(name, reason)| {
                // Produce a short reason by taking the first line.
                let short = reason.lines().next().unwrap_or(reason.as_str());
                format!("{name} — {short}")
            })
            .collect();

        println!(
            "{}",
            format!(
                "Installed {n_ok}/{total} plugins ({n_fail} failed: {})",
                failed_names.join(", ")
            )
            .yellow()
        );
    }

    Ok(())
}

// ── Single-plugin installation ────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn run_single(
    config: &Config,
    name: &str,
    requested_version: Option<&str>,
    registry: &Registry,
    format: Option<PluginFormat>,
    scope: Option<InstallScope>,
    from_file: Option<&Path>,
    dry_run: bool,
    batch_prefix: Option<&str>,
    json: bool,
) -> Result<()> {
    // ── Look up the plugin ────────────────────────────────────────────────────

    let plugin = registry.find(name).ok_or_else(|| {
        // Build "did you mean?" suggestions from registry slugs.
        let query_lower = name.to_lowercase();
        let prefix = if query_lower.len() >= 3 {
            &query_lower[..3]
        } else {
            &query_lower
        };

        let mut suggestions: Vec<&str> = registry
            .plugins
            .keys()
            .filter(|slug| {
                let slug_lower = slug.to_lowercase();
                slug_lower.starts_with(prefix) || slug_lower.contains(&query_lower)
            })
            .map(|s| s.as_str())
            .collect();
        suggestions.sort();
        suggestions.truncate(3);

        if suggestions.is_empty() {
            anyhow::anyhow!(ApmError::PluginNotFound {
                name: name.to_owned(),
            })
        } else {
            let suggestion_list = suggestions.join("', '");
            anyhow::anyhow!(
                "Plugin '{}' not found.\nHint: Did you mean '{}'? Try `apm search {}`.",
                name,
                suggestion_list,
                prefix
            )
        }
    })?;

    if !plugin.is_installable_product() {
        anyhow::bail!(
            "'{}' is a {} catalog item, not a standalone install target.\n\
             Hint: Use `apm info {}` or `apm open {}` for details. Use `apm bundles` for curated multi-plugin installs.",
            plugin.slug,
            plugin.product_type,
            plugin.slug,
            plugin.slug,
        );
    }

    let selected_release = plugin
        .resolve_release(requested_version)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Plugin '{}' does not have a registry release for version '{}'.\nHint: Available versions: {}",
                plugin.slug,
                requested_version.unwrap_or_default(),
                plugin.available_versions().join(", ")
            )
        })?;

    let selected_version = selected_release.version.clone();

    let mut selected_plugin = plugin.clone();
    selected_plugin.version = selected_release.version;
    selected_plugin.formats = selected_release.formats;

    let mut formats_to_check: Vec<_> = match format {
        Some(fmt) => {
            if let Some(src) = selected_plugin.formats.get(&fmt) {
                vec![(fmt, src)]
            } else {
                vec![]
            }
        }
        None => selected_plugin
            .formats
            .iter()
            .map(|(&f, s)| (f, s))
            .collect(),
    };
    formats_to_check.sort_by_key(|(f, _)| f.to_string());
    if formats_to_check.is_empty() {
        let available = selected_plugin
            .formats
            .keys()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let requested = format
            .map(|f| f.to_string())
            .unwrap_or_else(|| "any".to_string());
        anyhow::bail!(
            "Plugin '{}' does not have a {} format available in the registry.\n\
             Hint: Available formats: {}",
            plugin.slug,
            requested,
            if available.is_empty() {
                "(none listed)"
            } else {
                available.as_str()
            },
        );
    }

    // ── Check if already installed ────────────────────────────────────────────

    let mut state = InstallState::load(config)?;

    if let Some(existing) = state.find(&plugin.slug) {
        // If the user requested a specific format, check if it's already there.
        let already_has_format = match format {
            Some(fmt) => existing.formats.iter().any(|f| f.format == fmt),
            None => !existing.formats.is_empty(),
        };

        if already_has_format && existing.version == selected_version {
            if json {
                print_install_plan_json(
                    plugin,
                    "already_installed",
                    None,
                    &formats_to_check,
                    None,
                    format!(
                        "'{}' is already installed at version {}.",
                        plugin.slug, existing.version
                    ),
                )?;
                return Ok(());
            }
            if dry_run {
                println!(
                    "[dry-run] '{}' is already installed (v{}). Nothing to do.",
                    plugin.slug, existing.version
                );
            } else {
                println!(
                    "Plugin '{}' is already installed (v{}).",
                    plugin.slug, existing.version
                );
                println!("Use `apm upgrade {}` to update.", plugin.slug);
            }
            return Ok(());
        }
    }

    let is_managed = formats_to_check
        .iter()
        .any(|(_, src)| src.download_type == DownloadType::Managed);
    if is_managed {
        if json {
            return print_managed_json(plugin, registry, &formats_to_check, dry_run);
        }
        if dry_run {
            return print_managed_dry_run(plugin, registry, &formats_to_check);
        }
        if from_file.is_some() {
            println!(
                "Ignoring `--from-file` for {} because it is installed through a vendor manager.",
                plugin.name.bold()
            );
        }
        return handle_managed_install(plugin, registry);
    }

    // ── Check for manual download type (when no --from-file provided) ─────────

    if from_file.is_none() && !dry_run {
        let is_manual = formats_to_check
            .iter()
            .any(|(_, src)| src.download_type == DownloadType::Manual);

        if is_manual {
            let download_page = formats_to_check
                .iter()
                .find_map(|(_, src)| {
                    (!src.url.trim().is_empty() && src.url != "manual").then_some(src.url.as_str())
                })
                .or(plugin.homepage.as_deref());

            if json {
                print_install_plan_json(
                    plugin,
                    "manual_required",
                    None,
                    &formats_to_check,
                    None,
                    format!(
                        "{} requires manual installation. Install it externally, then run `apm scan`.",
                        plugin.name
                    ),
                )?;
                return Ok(());
            }

            println!("{} requires manual installation.\n", plugin.name.bold());
            if let Some(url) = download_page {
                println!("Opening the download page: {}", url.cyan());
            } else {
                println!("No download page is listed in the registry for this plugin.");
            }
            println!(
                "After installing {}, run {} to track it in apm.",
                plugin.name.bold(),
                "apm scan".bold()
            );

            if let Some(url) = download_page {
                std::process::Command::new("open")
                    .arg(url)
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
            }

            return Ok(());
        }
    }

    // ── Determine formats and install paths ───────────────────────────────────

    let effective_scope = scope.unwrap_or(config.install_scope);

    let mut formats_to_install: Vec<(PluginFormat, &apm_core::registry::FormatSource)> =
        match format {
            Some(fmt) => {
                if let Some(src) = selected_plugin.formats.get(&fmt) {
                    vec![(fmt, src)]
                } else {
                    vec![]
                }
            }
            None => selected_plugin
                .formats
                .iter()
                .map(|(&f, s)| (f, s))
                .collect(),
        };
    formats_to_install.sort_by_key(|(f, _)| f.to_string());

    let formats_to_show: Vec<String> = formats_to_install
        .iter()
        .map(|(f, _)| f.to_string())
        .collect();

    // ── Dry-run output ────────────────────────────────────────────────────────

    if dry_run {
        let install_base = install_destination_label(
            &formats_to_install
                .iter()
                .map(|(fmt, _)| *fmt)
                .collect::<Vec<_>>(),
            effective_scope,
        );

        if json {
            print_install_plan_json(
                plugin,
                "dry_run",
                Some(install_base),
                &formats_to_install,
                None,
                format!(
                    "Would install {} v{} ({}) to {}.",
                    plugin.name,
                    selected_plugin.version,
                    formats_to_show.join(", "),
                    install_base
                ),
            )?;
            return Ok(());
        }

        println!(
            "[dry-run] Would install {} v{} ({})",
            plugin.name.bold(),
            selected_plugin.version.cyan(),
            formats_to_show.join(", ")
        );
        println!("          Destination: {}", install_base.yellow());

        for (fmt, src) in &formats_to_install {
            let dl_type = match src.download_type {
                DownloadType::Direct => "direct download",
                DownloadType::Manual => "manual download required",
                DownloadType::Managed => "vendor installer required",
            };
            let source = match src.download_type {
                DownloadType::Manual => (!src.url.trim().is_empty() && src.url != "manual")
                    .then_some(src.url.as_str())
                    .or_else(|| {
                        plugin
                            .homepage
                            .as_deref()
                            .filter(|homepage| !homepage.trim().is_empty())
                    })
                    .unwrap_or("(no download URL listed)"),
                _ => src.url.as_str(),
            };
            println!(
                "          {}: {} ({})",
                fmt.to_string().cyan(),
                source,
                dl_type
            );
        }
        return Ok(());
    }

    if json {
        anyhow::bail!(
            "`apm install --json` only supports planning and external handoff flows right now.\n\
             Hint: Use `apm install --json <plugin> --dry-run`, or omit --json to install."
        );
    }

    // ── Show install plan ─────────────────────────────────────────────────────

    let prefix = batch_prefix.unwrap_or("");

    if let Some(path) = from_file {
        println!(
            "{prefix}Installing {} v{} ({}) from file {}...",
            plugin.name.bold(),
            selected_plugin.version.cyan(),
            formats_to_show.join(", "),
            path.display().to_string().yellow()
        );
    } else {
        println!(
            "{prefix}Installing {} v{} ({})...",
            plugin.name.bold(),
            selected_plugin.version.cyan(),
            formats_to_show.join(", ")
        );
    }

    // ── Install ───────────────────────────────────────────────────────────────

    crate::install::install_plugin(
        &selected_plugin,
        format,
        scope,
        config,
        &mut state,
        from_file,
    )
    .await
    .map_err(|e| {
        // Wrap with top-level context so the error shows the plugin name.
        e.context(format!("Failed to install '{}'", plugin.slug))
    })?;

    // ── Success message ───────────────────────────────────────────────────────

    let install_base = install_destination_label(
        &formats_to_install
            .iter()
            .map(|(fmt, _)| *fmt)
            .collect::<Vec<_>>(),
        effective_scope,
    );

    println!(
        "\n{}",
        format!(
            "Installed {} v{} to {}",
            plugin.name, selected_plugin.version, install_base
        )
        .green()
    );

    Ok(())
}

fn install_destination_label(formats: &[PluginFormat], scope: InstallScope) -> &'static str {
    let has_app = formats.contains(&PluginFormat::App);
    let has_plugin = formats
        .iter()
        .any(|fmt| matches!(fmt, PluginFormat::Au | PluginFormat::Vst3));

    match (has_app, has_plugin, scope) {
        (true, false, InstallScope::User) => "~/Applications/",
        (true, false, InstallScope::System) => "/Applications/",
        (false, true, InstallScope::User) => "~/Library/Audio/Plug-Ins/",
        (false, true, InstallScope::System) => "/Library/Audio/Plug-Ins/",
        _ => "format-specific destinations",
    }
}

fn handle_managed_install(
    plugin: &apm_core::registry::PluginDefinition,
    registry: &Registry,
) -> Result<()> {
    let installer_key = plugin.installer.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Plugin '{}' is marked as installer-managed but has no installer key.\nHint: Add `installer = \"...\"` to the registry entry.",
            plugin.slug
        )
    })?;

    let installer = registry.find_installer(installer_key).ok_or_else(|| {
        anyhow::anyhow!(
            "Installer '{}' for plugin '{}' was not found in the registry.\nHint: Run `apm sync` to refresh installers.toml.",
            installer_key,
            plugin.slug
        )
    })?;

    if let Some(app_path) = installed_app_path(installer) {
        println!(
            "Opening {} for {}...",
            installer.name.bold(),
            plugin.name.bold()
        );
        std::process::Command::new("open")
            .arg(&app_path)
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch {}: {e}", installer.name))?;
        println!(
            "Use {} to download and activate {}. Then run {} to track it in apm.",
            installer.name.bold(),
            plugin.name.bold(),
            "apm scan".bold()
        );
        return Ok(());
    }

    println!(
        "{} is required for {}.",
        installer.name.bold(),
        plugin.name.bold()
    );
    println!("Download it from: {}", installer.download_url.cyan());
    std::process::Command::new("open")
        .arg(&installer.download_url)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to open browser: {e}"))?;
    println!(
        "After installing {}, use it to install {}. Then run {} to track it in apm.",
        installer.name.bold(),
        plugin.name.bold(),
        "apm scan".bold()
    );
    Ok(())
}

fn installed_app_path(installer: &InstallerDefinition) -> Option<std::path::PathBuf> {
    installer
        .app_paths
        .iter()
        .find(|path| path.exists())
        .cloned()
}

fn print_install_plan_json(
    plugin: &PluginDefinition,
    status: &str,
    destination: Option<&str>,
    formats: &[(PluginFormat, &FormatSource)],
    installer: Option<InstallerJson<'_>>,
    message: String,
) -> Result<()> {
    let formats = formats
        .iter()
        .map(|(format, source)| InstallFormatJson {
            format: format.to_string(),
            download_type: source.download_type.to_string(),
            source: format_source_url(plugin, source),
        })
        .collect();

    let output = InstallPlanJson {
        plugin: &plugin.slug,
        name: &plugin.name,
        version: &plugin.version,
        status,
        destination,
        formats,
        installer,
        message,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn format_source_url(plugin: &PluginDefinition, source: &FormatSource) -> String {
    match source.download_type {
        DownloadType::Manual => (!source.url.trim().is_empty() && source.url != "manual")
            .then_some(source.url.as_str())
            .or_else(|| {
                plugin
                    .homepage
                    .as_deref()
                    .filter(|homepage| !homepage.trim().is_empty())
            })
            .unwrap_or("")
            .to_string(),
        _ => source.url.clone(),
    }
}

fn print_managed_json(
    plugin: &PluginDefinition,
    registry: &Registry,
    formats_to_check: &[(PluginFormat, &FormatSource)],
    dry_run: bool,
) -> Result<()> {
    let installer_key = plugin.installer.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Plugin '{}' is marked as installer-managed but has no installer key.\nHint: Add `installer = \"...\"` to the registry entry.",
            plugin.slug
        )
    })?;

    let installer = registry.find_installer(installer_key).ok_or_else(|| {
        anyhow::anyhow!(
            "Installer '{}' for plugin '{}' was not found in the registry.\nHint: Run `apm sync` to refresh installers.toml.",
            installer_key,
            plugin.slug
        )
    })?;

    let app_path = installed_app_path(installer);
    let installer_json = InstallerJson {
        key: &installer.key,
        name: &installer.name,
        download_url: &installer.download_url,
        homepage: &installer.homepage,
        installed_app_path: app_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
    };
    let status = if dry_run {
        "dry_run"
    } else if app_path.is_some() {
        "vendor_installer_available"
    } else {
        "vendor_installer_required"
    };
    let message = if app_path.is_some() {
        format!(
            "Use {} to download and activate {}. Then run `apm scan`.",
            installer.name, plugin.name
        )
    } else {
        format!(
            "{} is required for {}. Download it, install {}, then run `apm scan`.",
            installer.name, plugin.name, plugin.name
        )
    };

    print_install_plan_json(
        plugin,
        status,
        None,
        formats_to_check,
        Some(installer_json),
        message,
    )
}

fn print_managed_dry_run(
    plugin: &PluginDefinition,
    registry: &Registry,
    formats_to_check: &[(PluginFormat, &FormatSource)],
) -> Result<()> {
    let installer_key = plugin.installer.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "Plugin '{}' is marked as installer-managed but has no installer key.\nHint: Add `installer = \"...\"` to the registry entry.",
            plugin.slug
        )
    })?;

    let installer = registry.find_installer(installer_key).ok_or_else(|| {
        anyhow::anyhow!(
            "Installer '{}' for plugin '{}' was not found in the registry.\nHint: Run `apm sync` to refresh installers.toml.",
            installer_key,
            plugin.slug
        )
    })?;

    let formats = formats_to_check
        .iter()
        .map(|(fmt, _)| fmt.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    println!(
        "[dry-run] Would use {} for {} ({})",
        installer.name.bold(),
        plugin.name.bold(),
        formats
    );
    println!("          Download: {}", installer.download_url.yellow());
    println!("          Homepage: {}", installer.homepage.yellow());
    Ok(())
}
