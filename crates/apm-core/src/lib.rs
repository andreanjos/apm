pub mod bundle_id_store;
pub mod config;
pub mod error;
pub mod registry;
pub mod scanner;
pub mod state;

// Convenience re-exports
pub use config::Config;
pub use error::ApmError;
pub use registry::{PluginDefinition, PluginFormat, Registry};
pub use state::InstallState;
