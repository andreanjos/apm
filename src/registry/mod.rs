pub mod types;

// Re-export all registry types at the crate-module boundary so that future
// phases can import them as `use apm::registry::PluginDefinition` etc. without
// digging into the internal `types` submodule. The unused-imports warning is
// suppressed here because this is intentional public API surface.
#[allow(unused_imports)]
pub use types::{
    FormatSource, InstallType, PluginDefinition, PluginFormat, RegistryIndex, RegistryIndexEntry,
    Source,
};
