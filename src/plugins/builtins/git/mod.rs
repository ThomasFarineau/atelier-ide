//! Built-in Git plugin, modeled on IntelliJ's version control integration.

mod conflict;
mod diff;
pub mod github;
mod graph;
mod plugin;
pub(crate) mod repo;

pub use plugin::GitPlugin;
