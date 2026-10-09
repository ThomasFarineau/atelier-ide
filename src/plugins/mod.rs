//! Plugins: built-in ones compiled into the IDE (`builtin`, `builtins`) and external ones
//! declared by JSON manifests in a project (`external`).

pub mod builtin;
pub mod builtins;
mod external;

pub use external::*;
