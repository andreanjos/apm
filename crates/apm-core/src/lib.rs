pub mod config;
pub mod error;
pub mod license;
pub mod registry;
pub mod scanner;
pub mod state;

// Convenience re-exports
pub use config::Config;
pub use error::ApmError;
pub use license::{verify_signed_license, LicensePayload, LicenseStatus, SignedLicense};
pub use registry::{PluginDefinition, PluginFormat, Registry};
pub use state::InstallState;
