//! The IDE's own tool windows: Find in Files and Problems. They are part of the IDE as the
//! Project and Terminal windows are, not plugins: they are not listed in the plugins page and
//! cannot be turned off. They share the tool window interface of the built-in plugins
//! ([`crate::plugins::builtin::BuiltinPlugin`]), which reports them as `core`.

pub mod problems;
pub mod search;
