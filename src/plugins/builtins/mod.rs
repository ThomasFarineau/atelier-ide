//! Built-in plugins, compiled into the IDE. Each one implements [`crate::plugins::builtin::BuiltinPlugin`].

pub mod cargo;
pub mod csv;
pub mod git;
pub mod gradle;
pub mod image;
pub mod jupyter;
pub mod k8s;
pub mod markdown;
pub mod maven;
pub mod mcp;
pub mod mermaid;
pub mod msoffice;
pub mod node;
pub mod runtimes;
pub mod svg;
pub mod translations;
