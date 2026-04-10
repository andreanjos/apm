mod backup;
mod commands;
mod download;
mod install;
mod portable;
pub(crate) mod utils;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use apm_core::config::InstallScope;

// ── CLI Definition ────────────────────────────────────────────────────────────

/// apm — Audio Plugin Manager for macOS.
///
/// Manage AU and VST3 plugins from the command line, apt-style.
#[derive(Parser, Debug)]
#[command(
    name = "apm",
    version,
    about = "Audio Plugin Manager — apt-style management for macOS AU and VST3 plugins",
    long_about = None,
    propagate_version = true,
    after_help = "Quick start:\n  apm sync          Pull latest plugin registry\n  apm search synth  Search for plugins\n  apm install <id>  Install a plugin\n  apm list          See installed plugins"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output (equivalent to RUST_LOG=apm=debug).
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    /// Suppress non-error output (for scripting).
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    /// Output results as JSON instead of human-readable tables.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan macOS plugin directories and list all installed AU/VST3 plugins.
    ///
    /// Walks both system (/Library/Audio/Plug-Ins/) and user
    /// (~/Library/Audio/Plug-Ins/) directories, extracting metadata from each
    /// plugin bundle's Info.plist. Shows both apm-managed and unmanaged plugins.
    Scan {
        /// Show only plugins managed by apm.
        #[arg(long, conflicts_with = "unmanaged")]
        managed: bool,

        /// Show only plugins NOT managed by apm (third-party installs).
        #[arg(long, conflicts_with = "managed")]
        unmanaged: bool,
    },

    /// List all plugins installed by apm.
    ///
    /// Shows name, version, format, and install path for every plugin tracked
    /// in the local state file (~/.local/share/apm/state.toml).
    #[command(alias = "ls")]
    List {
        /// Filter by plugin format: "au" or "vst3".
        #[arg(long, short = 'f')]
        format: Option<String>,

        /// Sort by: "name" (default), "version", "date".
        #[arg(long, short = 's', default_value = "name")]
        sort: String,
    },

    /// Show detailed information about a plugin from the registry.
    ///
    /// Displays vendor, version, description, category, available formats,
    /// license, tags, and homepage URL.
    #[command(alias = "show")]
    Info {
        /// Plugin name or slug to look up (e.g. "tal-noisemaker").
        name: String,

        /// Show all available versions for this plugin.
        #[arg(long)]
        versions: bool,
    },

    /// Search the registry for plugins matching a query.
    ///
    /// Full-text match on plugin name, vendor, description, and tags.
    /// Run `apm sync` first to populate the local registry cache.
    /// Omit the query to list all plugins (optionally filtered by --category).
    #[command(alias = "s")]
    Search {
        /// Search query (e.g. "reverb", "tal", "free synth"). Omit to list all.
        query: Option<String>,

        /// Filter results by category (e.g. "instrument", "effect", "reverb").
        #[arg(long, short = 'c')]
        category: Option<String>,

        /// Filter results by vendor name (e.g. "Valhalla", "Fabfilter").
        #[arg(long)]
        vendor: Option<String>,

        /// Show only paid plugins.
        #[arg(long, conflicts_with = "free")]
        paid: bool,

        /// Show only free plugins.
        #[arg(long, conflicts_with = "paid")]
        free: bool,

        /// Filter by tag (e.g. "synth", "reverb", "open-source").
        #[arg(long, short = 't')]
        tag: Option<String>,

        /// Maximum number of results to show.
        #[arg(long, short = 'l')]
        limit: Option<usize>,

        /// Show only installed plugins in results.
        #[arg(long)]
        installed: bool,

        /// Show only plugins not currently installed.
        #[arg(long, conflicts_with = "installed")]
        new: bool,
    },

    /// Sync the local registry cache from configured Git remotes.
    ///
    /// Clones each registry on first run; performs fast-forward fetches on
    /// subsequent runs. Registry data is stored in ~/.cache/apm/registries/.
    /// Run this before `search`, `install`, or `outdated` to ensure you have
    /// the latest plugin definitions.
    #[command(alias = "update")]
    Sync,

    /// Download and install one or more plugins from the registry.
    ///
    /// Looks up each plugin in the synced registry, downloads the archive,
    /// verifies the SHA256 checksum, extracts, and places the bundle in the
    /// correct macOS plugin directory. Installs all available formats by
    /// default.
    ///
    /// Multiple plugins can be installed in one command:
    ///   apm install vital surge-xt dexed
    ///
    /// For plugins that require manual download (e.g. account signup), use
    /// --from-file to provide the downloaded archive directly (single plugin only).
    #[command(alias = "i", disable_version_flag = true)]
    Install {
        /// Plugin name(s) or slug(s) to install (e.g. "tal-noisemaker").
        #[arg(required_unless_present_any = ["from_file", "stdin"])]
        plugins: Vec<String>,

        /// Read plugin names from stdin (one per line or space-separated).
        ///
        /// Enables piping workflows like:
        ///   apm search --free --json | jq '.[].slug' -r | apm install --stdin
        ///   echo "vital surge-xt dexed" | apm install --stdin
        #[arg(long)]
        stdin: bool,

        /// Install a specific registry version instead of the latest release.
        #[arg(long = "version", value_name = "VERSION")]
        install_version: Option<String>,

        /// Install only this format: "au" or "vst3".
        #[arg(long, short = 'f', value_name = "FORMAT")]
        format: Option<String>,

        /// Install to the system directory (/Library/Audio/Plug-Ins/).
        /// Requires sudo. Default is user-scope (~/.Library/Audio/Plug-Ins/).
        #[arg(long)]
        system: bool,

        /// Install from a local file instead of downloading.
        ///
        /// Skips the download step and uses the provided archive path directly.
        /// SHA256 is still verified if the registry has a known checksum.
        /// Only valid when installing a single plugin.
        #[arg(long)]
        from_file: Option<PathBuf>,

        /// Show what would be installed without downloading or placing any files.
        #[arg(long)]
        dry_run: bool,

        /// Install a named bundle (meta-package) of plugins (e.g. "producer-essentials").
        ///
        /// Bundles are curated plugin collections. See `apm bundles` for available bundles.
        #[arg(long, value_name = "BUNDLE")]
        bundle: Option<String>,
    },

    /// Remove a plugin installed by apm.
    ///
    /// Deletes the plugin bundle(s) from disk and removes the entry from the
    /// local state file. Only removes plugins that apm installed.
    #[command(alias = "rm")]
    Remove {
        /// Plugin name or slug to remove (e.g. "tal-noisemaker").
        name: String,

        /// Show what would be removed without deleting anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Compare installed plugins against the registry and show a full diff.
    ///
    /// Shows three categories side by side: plugins with newer versions
    /// available (upgradeable), plugins no longer found in any configured
    /// registry (orphaned), and plugins that are already up to date.
    /// A superset of `apm outdated` -- use that command if you only care
    /// about available upgrades.
    #[command(alias = "d")]
    Diff,

    /// List installed plugins that have newer versions available in the registry.
    ///
    /// Compares installed versions from the local state file against the
    /// current registry. Pinned plugins are shown but marked as pinned and
    /// will not be touched by `apm upgrade`. Run `apm sync` first to ensure
    /// version data is current.
    #[command(alias = "od")]
    Outdated,

    /// Open a plugin's homepage in the default browser.
    ///
    /// Looks up the plugin in the registry and, if a homepage URL is listed,
    /// launches it with macOS `open` in your default browser. Handy for
    /// checking documentation, download pages, or changelogs before installing
    /// or upgrading.
    Open {
        /// Plugin name or slug to open (e.g. "vital").
        name: String,
    },

    /// Open the purchase page for a paid plugin.
    ///
    /// Looks up the plugin in the registry and opens the best purchase link
    /// in your default browser. For free plugins, suggests `apm install`
    /// instead.
    Buy {
        /// Plugin name or slug to purchase (e.g. "fabfilter-pro-q3").
        name: String,
    },

    /// Upgrade one or all plugins to the latest registry version.
    ///
    /// Without an argument, upgrades all outdated plugins except those that
    /// are pinned. With a plugin name, upgrades only that plugin.
    #[command(alias = "up")]
    Upgrade {
        /// Plugin name or slug to upgrade. Omit to upgrade all outdated plugins.
        name: Option<String>,

        /// Show what would be upgraded without making any changes.
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (for scripting/automation).
        #[arg(long)]
        yes: bool,
    },

    /// Pin a plugin to prevent it from being upgraded.
    ///
    /// Pinned plugins are skipped by `apm upgrade` and shown as pinned by
    /// `apm outdated`. Use --unpin to remove the pin. Use --list to show all
    /// pinned plugins.
    Pin {
        /// Plugin name or slug to pin or unpin. Omit when using --list.
        name: Option<String>,

        /// Remove the pin (allow the plugin to be upgraded again).
        #[arg(long, short = 'r')]
        unpin: bool,

        /// List all pinned plugins.
        #[arg(long, short = 'l')]
        list: bool,
    },

    /// View or locate the apm configuration.
    ///
    /// Use `config show` to display all current settings (registry URL,
    /// install scope, directories, sources), or `config path` to print the
    /// config file path for quick editing with your $EDITOR.
    #[command(subcommand, alias = "cfg")]
    Config(ConfigCommands),

    /// Manage registry sources.
    ///
    /// apm can pull plugin definitions from multiple Git-backed registries,
    /// similar to apt's sources.list. The official registry is configured
    /// by default.
    #[command(subcommand)]
    Sources(SourcesCommands),

    /// Generate shell completion scripts.
    ///
    /// Prints the completion script for the specified shell to stdout.
    /// Source or eval the output to enable tab-completion for apm.
    ///
    /// Examples:
    ///   apm completions zsh > ~/.zsh/completions/_apm
    ///   source <(apm completions bash)
    Completions {
        /// Shell to generate completions for: bash, zsh, fish, elvish, powershell.
        shell: String,
    },

    /// Run diagnostic checks on your apm installation.
    ///
    /// Verifies that plugin directories exist and are accessible, that the
    /// config and state files are valid, and that the registry cache is
    /// populated. Also scans for quarantined plugin bundles in user directories.
    Doctor,

    /// Print apm environment info for bug reports.
    ///
    /// Displays apm version, OS, architecture, and all relevant directory
    /// paths (config, data, cache, plugin directories). Useful for including
    /// in bug reports or verifying your setup.
    Env,

    /// Export the list of installed plugins to a file or stdout.
    ///
    /// Produces a portable setup string (apm1://) or legacy TOML/JSON file
    /// listing every plugin currently tracked by apm. Use this to migrate your
    /// setup to another machine with `apm import`.
    Export {
        /// Write output to this file instead of stdout.
        #[arg(long, short = 'o', value_name = "FILE")]
        output: Option<PathBuf>,

        /// Output format: "portable" (default), "toml" (legacy), or "json" (legacy).
        #[arg(long, default_value = "portable", value_name = "FORMAT")]
        format: String,
    },

    /// Import a plugin setup from a portable string or file.
    ///
    /// Accepts an `apm1://` portable string directly, a path to an `.apmsetup`
    /// file containing such a string, or a legacy TOML/JSON export file.
    /// Shows a preview of changes before proceeding.
    Import {
        /// Portable setup string (apm1://...) or path to export file.
        input: String,

        /// Preview what would change without making any modifications.
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (for scripting/automation).
        #[arg(long)]
        yes: bool,
    },

    /// Clean up the download cache to reclaim disk space.
    ///
    /// Scans the downloads cache directory (~/.cache/apm/downloads/), reports
    /// total size, and removes all cached archives. Safe to run at any time --
    /// plugins already installed are not affected. Use --dry-run to preview.
    Cleanup {
        /// Show what would be deleted without actually removing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// List and inspect plugin bundles (curated collections).
    ///
    /// Bundles group related plugins into themed collections for one-command
    /// installation. Without an argument, lists all available bundles. Pass a
    /// bundle name to see its description and included plugins.
    /// Use `apm install --bundle <name>` to install all plugins in a bundle.
    Bundles {
        /// Show details for a specific bundle (e.g. "producer-essentials").
        name: Option<String>,
    },

    /// List all plugin categories and subcategories with plugin counts.
    ///
    /// Shows a tree of categories from the synced registry, with the number
    /// of plugins in each category and subcategory. Useful for discovering
    /// what kinds of plugins are available before drilling down with
    /// `apm search --category <name>`.
    #[command(alias = "cats")]
    Categories,

    /// Verify the integrity of an installed plugin on disk.
    ///
    /// Checks that each installed format bundle (AU, VST3) still exists at
    /// the recorded path, has not been moved or deleted, and is not
    /// quarantined by macOS Gatekeeper. Reports a per-format status table
    /// and an overall health verdict (healthy / degraded / broken).
    #[command(alias = "verify")]
    Check {
        /// Plugin name or slug to verify (e.g. "tal-noisemaker").
        name: String,
    },

    /// Print a plugin count as a plain integer (for scripting and prompts).
    ///
    /// Outputs a single number with no decoration, ideal for shell prompts,
    /// scripts, or CI checks. With no flags, prints the installed count.
    /// With --available, prints the registry count instead.
    /// With --json, outputs both counts as a JSON object.
    ///
    /// Examples:
    ///   echo "$(apm count) plugins installed"
    ///   PS1="[apm:$(apm count)] $ "
    #[command(alias = "c")]
    Count {
        /// Count available registry plugins instead of installed plugins.
        #[arg(long)]
        available: bool,
    },

    /// Show install details and provenance for a plugin.
    ///
    /// Displays when the plugin was installed, which version, which registry
    /// source it came from, whether it is pinned, and the on-disk paths for
    /// each installed format. Useful for auditing how a plugin ended up on
    /// your system or troubleshooting version mismatches.
    Why {
        /// Plugin name or slug to inspect (e.g. "tal-noisemaker").
        name: String,
    },

    /// Restore a plugin to its previous version from a local backup.
    ///
    /// Backups are created automatically before each `apm upgrade`. This
    /// command restores the most recent backup snapshot for the given plugin.
    /// It does NOT re-download from the registry -- use
    /// `apm install <plugin> --version <x.y.z>` to fetch a specific version.
    /// Use --list to see all available backups with their dates and sizes.
    Rollback {
        /// Plugin name or slug to roll back (e.g. "valhalla-supermassive").
        plugin: Option<String>,

        /// List all available backups with dates and sizes.
        #[arg(long, short = 'l')]
        list: bool,
    },

    /// Discover something new -- suggest a random plugin from the registry.
    ///
    /// Picks a random plugin and displays its full info card. Great for
    /// discovering plugins you might not have found through search. Optionally
    /// filter by category to narrow the suggestion to instruments, effects, etc.
    Random {
        /// Limit to a category (e.g. "instrument", "effect", "reverb").
        #[arg(long, short = 'c')]
        category: Option<String>,
    },

    /// Show disk usage of installed plugins, sorted largest first.
    ///
    /// Walks each installed plugin's bundle directories and sums file sizes.
    /// Displays a per-plugin table with AU and VST3 sizes, sorted by total
    /// size (largest first), plus a grand total at the bottom. Useful for
    /// finding which plugins consume the most disk space.
    #[command(alias = "du")]
    Size,

    /// Show a dashboard summary of your apm environment.
    ///
    /// Displays at a glance: installed plugin count with AU/VST3 format
    /// breakdown, total available plugins in the registry, number of pinned
    /// plugins, configured registry sources, download cache size on disk,
    /// and the timestamp of the last `apm sync`.
    #[command(alias = "st")]
    Stats,

    /// Show plugin install and upgrade history sorted by date.
    ///
    /// Lists all installed plugins in reverse chronological order (most
    /// recent first) based on their install or last-upgrade timestamp.
    /// Useful for reviewing what changed recently or auditing when a
    /// particular plugin was added.
    #[command(alias = "log")]
    History {
        /// Maximum number of entries to show (default: all).
        #[arg(long, short = 'l')]
        limit: Option<usize>,
    },

    /// List all unique tags across the registry with occurrence counts.
    ///
    /// Collects every tag from every plugin definition, counts how often each
    /// appears, and displays the top 50 in a compact word-cloud layout sorted
    /// by frequency. Use --json to get the full untruncated list.
    /// Pair with `apm search --tag <name>` to find plugins by tag.
    Tags,

    /// List all plugin vendors in the registry with plugin counts.
    ///
    /// Shows every vendor that has at least one plugin in the synced registry,
    /// sorted by number of plugins (most first). Useful for discovering who
    /// publishes the most plugins. Use `apm search --vendor <name>` to browse
    /// a specific vendor's catalog.
    Vendors,

    /// List registry plugins that you have not yet installed.
    ///
    /// The inverse of `apm list` -- shows everything available in the
    /// registry minus what you already have. Useful for browsing what is
    /// left to try. Optionally filter by category or cap the output.
    #[command(alias = "available")]
    Uninstalled {
        /// Filter by category (e.g. "instrument", "effect", "reverb").
        #[arg(long, short = 'c')]
        category: Option<String>,

        /// Maximum number of results to show (default: all).
        #[arg(long, short = 'l')]
        limit: Option<usize>,
    },

    /// Print the apm version and exit.
    ///
    /// Shows the version string (e.g. "apm 0.1.0"). With --json, includes
    /// the build target triple alongside the version.
    #[command(alias = "v")]
    Version,
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Show current configuration values.
    ///
    /// Displays the registry URL, install scope, config file path, data and
    /// cache directories, and configured sources.
    Show,

    /// Print the config file path (for use with $EDITOR).
    ///
    /// Outputs the absolute path to the apm configuration file so you can
    /// quickly open it in your editor: `$EDITOR $(apm config path)`
    Path,
}

#[derive(Subcommand, Debug)]
enum SourcesCommands {
    /// Add a new registry source.
    ///
    /// The URL must point to a Git repository following the apm registry
    /// compatibility format (an index.toml with a plugins/ directory of TOML files).
    Add {
        /// Git repository URL of the registry to add.
        url: String,

        /// Short name for this source (derived from URL if omitted).
        #[arg(long, short = 'n')]
        name: Option<String>,
    },

    /// Remove a registry source by name.
    ///
    /// Deletes the named source from the config. The local cache for this
    /// registry is not removed; run `apm cleanup` to reclaim space.
    Remove {
        /// Name of the source to remove (see `apm sources list`).
        name: String,
    },

    /// List all configured registry sources.
    ///
    /// Shows each source's short name and Git URL. The first entry is the
    /// default (official) registry.
    List,
}

// ── Entry Point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Respect the NO_COLOR standard (https://no-color.org/).
    // When set, disable all colored output — useful in CI and piped contexts.
    if std::env::var("NO_COLOR").is_ok() {
        colored::control::set_override(false);
    }

    if let Err(e) = run().await {
        eprintln!("error: {e}");
        // Walk the error chain for additional context.
        let mut source = e.source();
        while let Some(cause) = source {
            eprintln!("  caused by: {cause}");
            source = cause.source();
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing.
    // --quiet (when not combined with --json) suppresses everything below error.
    // --verbose sets the level to debug for the apm crate.
    // Otherwise fall back to RUST_LOG, then to warn.
    let env_filter = if cli.quiet && !cli.json {
        EnvFilter::new("error")
    } else if cli.verbose {
        EnvFilter::new("apm=debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .init();

    // Initialise config directory and load configuration.
    // This creates ~/.config/apm/ on first run.
    let config = apm_core::config::init()?;

    let json = cli.json;
    let quiet = cli.quiet;

    // Dispatch to command handlers.
    match &cli.command {
        Commands::Scan { managed, unmanaged } => {
            commands::scan::run(&config, json, *managed, *unmanaged).await
        }

        Commands::List { format, sort } => {
            commands::list::run(&config, json, format.as_deref(), sort).await
        }

        Commands::Info { name, versions } => {
            commands::info::run(&config, name, json, *versions).await
        }

        Commands::Search {
            query,
            category,
            vendor,
            paid,
            free,
            tag,
            limit,
            installed,
            new,
        } => {
            let q = query.as_deref().unwrap_or("");
            commands::search::run(
                &config,
                q,
                category.as_deref(),
                vendor.as_deref(),
                *paid,
                *free,
                tag.as_deref(),
                *limit,
                *installed,
                *new,
                json,
            )
            .await
        }

        Commands::Sync => commands::sync_cmd::run(&config, json, quiet).await,

        Commands::Install {
            plugins,
            stdin,
            install_version,
            format,
            system,
            from_file,
            dry_run,
            bundle,
        } => {
            let plugin_format = match format.as_deref() {
                Some("au") => Some(apm_core::registry::PluginFormat::Au),
                Some("vst3") => Some(apm_core::registry::PluginFormat::Vst3),
                Some(other) => {
                    anyhow::bail!(
                        "Unknown format '{other}'. Valid values are: au, vst3.\n\
                         Hint: Use `--format au` or `--format vst3`, or omit the flag to \
                         install all available formats."
                    )
                }
                None => None,
            };
            let scope = if *system {
                Some(InstallScope::System)
            } else {
                None
            };
            commands::install::run(
                &config,
                plugins,
                *stdin,
                install_version.as_deref(),
                plugin_format,
                scope,
                from_file.as_deref(),
                *dry_run,
                bundle.as_deref(),
            )
            .await
        }

        Commands::Remove { name, dry_run } => {
            commands::remove::run(&config, name, json, *dry_run).await
        }

        Commands::Diff => commands::diff::run(&config, json).await,

        Commands::Outdated => commands::outdated::run(&config, json).await,

        Commands::Open { name } => commands::open::run(&config, name).await,

        Commands::Buy { name } => commands::buy::run(&config, name).await,

        Commands::Upgrade { name, dry_run, yes } => {
            commands::upgrade::run(&config, name.as_deref(), *dry_run, json, *yes).await
        }

        Commands::Pin { name, unpin, list } => {
            commands::pin::run(&config, name.as_deref(), *unpin, *list, json).await
        }

        Commands::Config(sub) => match sub {
            ConfigCommands::Show => commands::config_cmd::run_show(&config, json),
            ConfigCommands::Path => commands::config_cmd::run_path(json),
        },

        Commands::Sources(sub) => match sub {
            SourcesCommands::Add { url, name } => {
                commands::sources::run_add(&config, url, name.as_deref()).await
            }
            SourcesCommands::Remove { name } => commands::sources::run_remove(&config, name).await,
            SourcesCommands::List => commands::sources::run_list(&config).await,
        },

        Commands::Completions { shell } => commands::completions::run(shell),

        Commands::Doctor => commands::doctor::run(&config, json),

        Commands::Env => commands::env_cmd::run(json),

        Commands::Export { output, format } => {
            commands::export_cmd::run(&config, output.as_ref(), format).await
        }

        Commands::Import {
            input,
            dry_run,
            yes,
        } => commands::import_cmd::run(&config, input, *dry_run, *yes).await,

        Commands::Cleanup { dry_run } => commands::cleanup::run(&config, *dry_run, json).await,

        Commands::Bundles { name } => commands::bundles::run(&config, name.as_deref(), json).await,

        Commands::Categories => commands::categories::run(&config, json).await,

        Commands::Check { name } => commands::check::run(&config, name, json).await,

        Commands::Count { available } => commands::count::run(&config, json, *available).await,

        Commands::Why { name } => commands::why::run(&config, name, json).await,

        Commands::Rollback { plugin, list } => {
            commands::rollback::run(&config, plugin.as_deref(), *list, json).await
        }

        Commands::Random { category } => {
            commands::random::run(&config, category.as_deref(), json).await
        }

        Commands::Size => commands::size::run(&config, json).await,

        Commands::Stats => commands::stats::run(&config, json).await,

        Commands::History { limit } => commands::history::run(&config, *limit, json).await,

        Commands::Tags => commands::tags::run(&config, json).await,

        Commands::Vendors => commands::vendors::run(&config, json).await,

        Commands::Uninstalled { category, limit } => {
            commands::uninstalled::run(&config, category.as_deref(), *limit, json).await
        }

        Commands::Version => {
            let version = env!("CARGO_PKG_VERSION");
            if json {
                let target = format!("{}-apple-{}", std::env::consts::ARCH, std::env::consts::OS);
                let obj = serde_json::json!({
                    "version": version,
                    "target": target,
                });
                println!("{}", serde_json::to_string_pretty(&obj)?);
            } else {
                println!("apm {version}");
            }
            Ok(())
        }
    }
}
