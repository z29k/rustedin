//! One module per platform.
//!
//! Each provider owns its authentication, its HTTP conventions and its
//! commands, and shares nothing with the others but [`crate::core`]. Adding a
//! platform means adding a module here, a section to the config schema and a
//! subcommand group to the CLI — nothing else moves.

#[cfg(feature = "linkedin")]
pub mod linkedin;
#[cfg(feature = "meta")]
pub mod meta;
