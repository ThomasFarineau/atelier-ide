//! Built-in Git plugin, modeled on IntelliJ's version control integration.

mod conflict;
mod diff;
mod graph;
mod plugin;
pub(crate) mod repo;

pub use plugin::GitPlugin;
