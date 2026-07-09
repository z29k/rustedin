# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Pinned toolchain via `rust-toolchain.toml` so local and CI use the same Rust /
  clippy version (no toolchain drift).
- `scripts/check.sh` to run the exact CI checks locally in one command.
- Opt-in `.githooks/pre-push` hook that blocks a push when the checks fail
  (`git config core.hooksPath .githooks`).

### Changed

- Bumped dependencies: `rand` 0.8 → 0.9 (migrated to the new `rng()` / `random()`
  API), `tokio` 1.52.3, `clap` 4.6.1, `serde_json` 1.0.150, `open` 5.3.6.
- CI: bumped `actions/checkout` to v7 (runs on Node.js 24, resolves the Node.js 20
  deprecation warning) and now installs the pinned toolchain via `rustup show`.

## [1.0.0] - 2026-07-09

### Added

- Multi-account management for LinkedIn personal accounts and company pages.
- `setup` — configure personal / organization app credentials.
- `auth` — OAuth 2.0 authentication with an automatic browser flow and a local
  callback listener.
- `accounts` / `status` — list accounts and inspect token expiry.
- `post` — publish a text post with configurable visibility.
- `reshare` — reshare a post from one or many accounts concurrently (with `*`
  wildcard for all personal accounts).
- `share` — share an external article with a link-preview card or a full-size
  image, including optional thumbnail from a local file or URL.
- `get-post` / `comments` / `profile` — read helpers.
- Automatic access-token refresh 7 days before expiry.
- Bilingual documentation (English and French).

[Unreleased]: https://github.com/z29k/rustedin/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/z29k/rustedin/releases/tag/v1.0.0
