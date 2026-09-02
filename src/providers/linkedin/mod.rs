//! LinkedIn personal accounts and company pages.
//!
//! LinkedIn requires **two** apps: the Community Management API must be the
//! only product on the app that posts for a company page, which leaves personal
//! posting to an app of its own. `setup --app=personal|organization` configures
//! each, and an account records which one authorized it.

pub mod api;
pub mod auth;
pub mod commands;
pub mod publish;
pub mod store;
