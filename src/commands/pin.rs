// pin command — pin or unpin a plugin, or list all pinned plugins.

use anyhow::Result;

use crate::config::Config;
use crate::state::InstallState;

pub async fn run(config: &Config, name: Option<&str>, unpin: bool, list: bool) -> Result<()> {
    let mut state = InstallState::load(config)?;

    // ── List mode ─────────────────────────────────────────────────────────────

    if list {
        let pinned: Vec<_> = state.plugins.iter().filter(|p| p.pinned).collect();

        if pinned.is_empty() {
            println!("No pinned plugins.");
            return Ok(());
        }

        let col_name = pinned
            .iter()
            .map(|p| p.name.len())
            .max()
            .unwrap_or(6)
            .max(6);

        println!(
            "{:<col_name$}  Version",
            "Plugin",
            col_name = col_name
        );
        println!("{}", "\u{2500}".repeat(col_name + 2 + 7));

        for plugin in &pinned {
            println!(
                "{:<col_name$}  {}",
                plugin.name,
                plugin.version,
                col_name = col_name,
            );
        }

        return Ok(());
    }

    // ── Pin / unpin mode ──────────────────────────────────────────────────────

    let plugin_name = match name {
        Some(n) => n,
        None => {
            anyhow::bail!(
                "Plugin name required.\n\
                 Usage: apm pin <plugin>       — pin a plugin\n\
                 Usage: apm pin -r <plugin>    — unpin a plugin\n\
                 Usage: apm pin --list         — list all pinned plugins"
            );
        }
    };

    // Check the plugin is installed.
    let plugin = match state.find(plugin_name) {
        Some(p) => p.clone(),
        None => {
            println!(
                "Plugin '{}' is not installed. Install it first with `apm install {}`.",
                plugin_name, plugin_name
            );
            return Ok(());
        }
    };

    if unpin {
        // Unpin.
        if let Some(p) = state.find_mut(plugin_name) {
            p.pinned = false;
        }
        state.save(config)?;
        println!("Unpinned {}", plugin.name);
    } else {
        // Pin.
        if let Some(p) = state.find_mut(plugin_name) {
            p.pinned = true;
        }
        state.save(config)?;
        println!("Pinned {} at v{}", plugin.name, plugin.version);
    }

    Ok(())
}
