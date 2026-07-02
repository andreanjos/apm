use std::path::PathBuf;

use crate::registry::{FormatSource, PluginFormat};

pub(super) struct ReadyArchiveFormat {
    pub(super) format: PluginFormat,
    pub(super) source: FormatSource,
}

pub(super) struct ArchiveFormat {
    pub(super) format: PluginFormat,
    pub(super) source: FormatSource,
    pub(super) archive_path: PathBuf,
}
