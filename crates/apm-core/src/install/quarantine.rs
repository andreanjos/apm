use std::path::Path;

use anyhow::Result;
use tracing::{debug, info};

/// Remove the macOS quarantine extended attribute from an installed bundle.
///
/// Missing xattrs and unavailable `xattr` are treated as non-fatal because the
/// bundle is still usable on systems that never applied the flag.
pub fn remove_quarantine(path: &Path) -> Result<()> {
    info!("Stripping com.apple.quarantine from {}", path.display());

    let output = std::process::Command::new("xattr")
        .args(["-rd", "com.apple.quarantine"])
        .arg(path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            debug!("Quarantine xattr removed from {}", path.display());
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            debug!(
                "xattr exited {} for {} (the quarantine flag may not be present): {}",
                out.status,
                path.display(),
                stderr.trim()
            );
        }
        Err(error) => {
            debug!(
                "Could not run xattr on {}: {error} (continuing)",
                path.display()
            );
        }
    }

    Ok(())
}
