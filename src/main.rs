mod commands;
mod config;
mod error;
mod registry;
mod scanner;
mod state;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use crate::config::InstallScope;

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
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output (equivalent to RUST_LOG=apm=debug).
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan macOS plugin directories and list all installed AU/VST3 plugins.
    ///
    /// Walks both system (/Library/Audio/Plug-Ins/) and user
    /// (~/Library/Audio/Plug-Ins/) directories, extracting metadata from each
    /// plugin bundle's Info.plist. Shows both apm-managed and unmanaged plugins.
    Scan,

    /// List all plugins installed by apm.
    ///
    /// Shows name, version, format, and install path for every plugin tracked
    /// in the local state file (~/.local/share/apm/state.toml).
    List,

    /// Show detailed information about a plugin from the registry.
    ///
    /// Displays vendor, version, description, category, available formats,
    /// license, tags, and homepage URL.
    Info {
        /// Plugin name or slug to look up (e.g. "tal-noisemaker").
        name: String,
    },

    /// Search the registry for plugins matching a query.
    ///
    /// Full-text match on plugin name, vendor, description, and tags.
    /// Run `apm sync` first to populate the local registry cache.
    Search {
        /// Search query (e.g. "reverb", "tal", "free synth").
        query: String,

        /// Filter results by category (e.g. "instrument", "effect", "reverb").
        #[arg(long, short = 'c')]
        category: Option<String>,
    },

    /// Sync the local registry cache from the configured Git remote.
    ///
    /// Clones the registry on first run; fast-forward fetches on subsequent
    /// runs. Registry is stored in ~/.cache/apm/registries/.
    Sync,

    /// Download and install a plugin from the registry.
    ///
    /// Looks up the plugin in the synced registry, downloads the archive,
    /// verifies the SHA256 checksum, extracts, and places the bundle in the
    /// correct macOS plugin directory. Installs all available formats by
    /// default.
    Install {
        /// Plugin name or slug to install (e.g. "tal-noisemaker").
        name: String,

        /// Install only this format: "au" or "vst3".
        #[arg(long, short = 'f', value_name = "FORMAT")]
        format: Option<String>,

        /// Install to the system directory (/Library/Audio/Plug-Ins/).
        /// Requires sudo. Default is user-scope (~/.Library/Audio/Plug-Ins/).
        #[arg(long)]
        system: bool,
    },

    /// Remove a plugin installed by apm.
    ///
    /// Deletes the plugin bundle(s) from disk and removes the entry from the
    /// local state file. Only removes plugins that apm installed.
    Remove {
        /// Plugin name or slug to remove (e.g. "tal-noisemaker").
        name: String,
    },

    /// List installed plugins that have newer versions available in the registry.
    ///
    /// Compares installed versions from the local state file against the
    /// current registry. Pinned plugins are shown but marked as pinned.
    Outdated,

    /// Upgrade one or all plugins to the latest registry version.
    ///
    /// Without an argument, upgrades all outdated plugins except those that
    /// are pinned. With a plugin name, upgrades only that plugin.
    Upgrade {
        /// Plugin name or slug to upgrade. Omit to upgrade all outdated plugins.
        name: Option<String>,
    },

    /// Pin a plugin to prevent it from being upgraded.
    ///
    /// Pinned plugins are skipped by `apm upgrade` and shown as pinned by
    /// `apm outdated`. Use --unpin to remove the pin.
    Pin {
        /// Plugin name or slug to pin or unpin.
        name: String,

        /// Remove the pin (allow the plugin to be upgraded again).
        #[arg(long)]
        unpin: bool,
    },

    /// Manage registry sources.
    ///
    /// apm can pull plugin definitions from multiple Git-backed registries,
    /// similar to apt's sources.list. The official registry is configured
    /// by default.
    #[command(subcommand)]
    Sources(SourcesCommands),
}

#[derive(Subcommand, Debug)]
enum SourcesCommands {
    /// Add a new registry source.
    ///
    /// The URL must point to a Git repository following the apm registry
    /// format (an index.toml with a plugins/ directory of TOML files).
    Add {
        /// Git repository URL of the registry to add.
        url: String,

        /// Short name for this source (derived from URL if omitted).
        #[arg(long, short = 'n')]
        name: Option<String>,
    },

    /// Remove a registry source by name.
    Remove {
        /// Name of the source to remove (see `apm sources list`).
        name: String,
    },

    /// List all configured registry sources.
    List,
}

// ── Entry Point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
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

    // Initialise tracing. --verbose sets the level to debug for the apm crate;
    // otherwise fall back to RUST_LOG, then to warn.
    let env_filter = if cli.verbose {
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
    let config = config::init()?;

    // Dispatch to command handlers.
    match &cli.command {
        Commands::Scan => commands::scan::run(&config).await,

        Commands::List => commands::list::run(&config).await,

        Commands::Info { name } => commands::info::run(&config, name).await,

        Commands::Search { query, category } => {
            commands::search::run(&config, query, category.as_deref()).await
        }

        Commands::Sync => commands::sync_cmd::run(&config).await,

        Commands::Install {
            name,
            format,
            system,
        } => {
            let plugin_format = match format.as_deref() {
                Some("au") => Some(registry::PluginFormat::Au),
                Some("vst3") => Some(registry::PluginFormat::Vst3),
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
            commands::install::run(&config, name, plugin_format, scope).await
        }

        Commands::Remove { name } => commands::remove::run(&config, name).await,

        Commands::Outdated => commands::outdated::run(&config).await,

        Commands::Upgrade { name } => {
            commands::upgrade::run(&config, name.as_deref()).await
        }

        Commands::Pin { name, unpin } => {
            commands::pin::run(&config, name, *unpin).await
        }

        Commands::Sources(sub) => match sub {
            SourcesCommands::Add { url, name } => {
                commands::sources::run_add(&config, url, name.as_deref()).await
            }
            SourcesCommands::Remove { name } => {
                commands::sources::run_remove(&config, name).await
            }
            SourcesCommands::List => commands::sources::run_list(&config).await,
        },
    }
}
