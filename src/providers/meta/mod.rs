//! Facebook Pages and Instagram Professional accounts, both driven through the
//! Meta Graph API.
//!
//! The two platforms share one authorization: the Page tokens a Facebook Login
//! yields publish to Pages, and each Page carries the ID of the Instagram
//! account linked to it. That is why account management lives under `meta`
//! while publishing is split between `facebook` and `instagram`.

pub mod api;
pub mod auth;
pub mod commands;
pub mod facebook;
pub mod instagram;
pub mod store;
