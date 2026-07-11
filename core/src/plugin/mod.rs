pub mod manager;
pub mod metadata;
pub mod sdk;

pub use manager::PluginManager;
pub use metadata::{PluginMetadata, PluginMetadataBuilder};
pub use sdk::{Plugin, PluginContext};
