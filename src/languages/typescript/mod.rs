//! TypeScript support. The engine is the one of JavaScript (`languages::javascript`): the same
//! syntax tree analysis, completion, hover, navigation and language server, with the TypeScript
//! grammar. This module holds what is specific to TypeScript: the types written in the code
//! (`types`) and the keywords, library types and templates (`builtins`).

pub mod builtins;
pub mod highlight;
pub mod types;

#[cfg(test)]
#[path = "../../../test/languages/typescript/tests.rs"]
mod tests;
