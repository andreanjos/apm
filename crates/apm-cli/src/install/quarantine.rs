// Quarantine removal — strips the com.apple.quarantine extended attribute from
// a plugin bundle and all its contents. This is a hard requirement on macOS
// Sequoia (15+) where DAWs refuse to load quarantined plugins.

use std::path::Path;

use anyhow::Result;
use tracing::{debug, info};

/// Remove the `com.apple.quarantine` xattr from `path` recursively.
///
/// Uses `xattr -rd com.apple.quarantine <path>` which walks all files inside
/// the bundle. Non-zero exit is treated as a warning (the attribute may simply
/// not be present) rather than a hard error.
pub fn remove_quarantine(path: &Path) -> Result<()> {
    info!(
        "Stripping com.apple.quarantine from {}",
        path.display()
    );

    let output = std::process::Command::new("xattr")
        .args(["-rd", "com.apple.quarantine"])
        .arg(path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            debug!(
                "Quarantine xattr removed from {}",
                path.display()
            );
        }
        Ok(out) => {
            // Non-zero exit — either the attribute was not present (harmless)
            // or xattr had a minor issue. Log at debug level and continue.
            let stderr = String::from_utf8_lossy(&out.stderr);
            debug!(
                "xattr exited {} for {} (may be fine — quarantine flag may not be present): {}",
                out.status,
                path.display(),
                stderr.trim()
            );
        }
        Err(e) => {
            // xattr binary not found or could not be spawned. Very unusual on macOS.
            debug!(
                "Could not run xattr on {}: {e} (continuing anyway)",
                path.display()
            );
        }
    }

    Ok(())
}
