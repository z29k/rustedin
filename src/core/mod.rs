//! Platform-agnostic plumbing shared by every provider.
//!
//! Everything here is deliberately ignorant of LinkedIn, Facebook and
//! Instagram: the config file layout, the HTTP client, the OAuth loopback
//! server, media inputs and the output contract are defined exactly once, and
//! each provider in [`crate::providers`] builds on top of them.

pub mod config;
pub mod http;
pub mod media;
pub mod oauth;
pub mod output;
