//! Plugins: built-in ones compiled into the IDE (`builtin`, `builtins`) and external ones
//! declared by JSON manifests in a project (`external`), which may bring a dynamic library
//! (`native`).

pub mod builtin;
pub mod builtins;
mod external;
pub mod native;

pub use external::*;
