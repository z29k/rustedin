# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0] - 2026-09-01

rustedin becomes multi-platform. It absorbs `rustameta` — a sibling CLI for
Facebook Pages and Instagram — into a single binary, a single config file and a
single OAuth story, and grows a `broadcast` command that publishes the same
content to all of them at once.

### Added

- **Facebook Pages**: `facebook post` (text and/or link), `facebook photo`
  (single photo, or multi-photo via `attached_media`), `facebook video`, all
  with `--schedule` (validated against Meta's 10-minute / 75-day window) and
  `--draft`.
- **Instagram**: `instagram post` (image, video, or a 2–10 item carousel),
  `instagram reel`, `instagram story`, driven by the container → poll → publish
  handshake. Local images are relayed through the linked Facebook Page as
  unpublished photos so the container can ingest their CDN URL; local videos use
  the resumable upload protocol. `instagram publish` recovers a container whose
  processing outlasted the poll timeout, and `instagram limit` reports the
  24-hour publishing quota.
- **Meta account management**: `meta setup`, `meta auth` (one Facebook Login
  covers both platforms), `meta token` to adopt a Business system user token,
  `meta pages --refresh`, `meta use` to pin a default Page, `meta get` as a raw
  Graph API escape hatch. `appsecret_proof` is sent on every call.
- **`broadcast`** — the same `--text` / `--image` / `--link` fanned out to
  several `platform:account[/page]` targets in parallel, mapped onto what each
  platform actually accepts. Failures are per target, `--dry-run` prints the
  plan without publishing, and the process exits non-zero if any target failed.
- **`accounts` and `status` at the root**, aggregating every platform;
  `--check` validates Meta tokens against `/debug_token`.
- **`migrate --from <file>`** — folds another config file into this one. Written
  for the 1.x → 2.0 move: point it at a `rustameta.json` and its app credentials
  and accounts land under the `meta` key. Nothing already configured is
  overwritten unless `--force` says so.
- **Cargo features** `linkedin` and `meta` (both on by default) to build a
  smaller, single-platform binary. CI checks each one on its own.
- Retries with exponential backoff on LinkedIn calls, which previously had none.
- Actionable LinkedIn errors: the `serviceErrorCode` envelope is reduced to one
  line, with a hint naming the fix for 401, 403, 422, 426 and 429.
- `linkedin auth --port` to move the OAuth callback off a busy port 8765.

### Changed

- **Breaking — the CLI is grouped by platform.** Every 1.x command moves under
  `linkedin` (aliases: `li`, `fb`, `ig`, for the other groups):

  | 1.x | 2.0 |
  |-----|-----|
  | `rustedin setup --app=…` | `rustedin linkedin setup --app=…` |
  | `rustedin auth --account=…` | `rustedin linkedin auth --account=…` |
  | `rustedin accounts` | `rustedin linkedin accounts` (or root `accounts` for every platform) |
  | `rustedin status` | `rustedin linkedin status` (or root `status`) |
  | `rustedin post` | `rustedin linkedin post` |
  | `rustedin reshare` | `rustedin linkedin reshare` |
  | `rustedin share` | `rustedin linkedin share` |
  | `rustedin get-post` | `rustedin linkedin get-post` |
  | `rustedin comments` | `rustedin linkedin comments` |
  | `rustedin profile` | `rustedin linkedin profile` |

- **Breaking — the config file is namespaced per platform**, so the same alias
  can name a LinkedIn company page and a Meta account without colliding:
  `linkedInApp` and the top-level `accounts` map move under `linkedin.app` and
  `linkedin.accounts`. **1.x files are upgraded automatically on load**; nothing
  to do by hand. Unknown top-level keys now survive a rewrite.
- A malformed config file is an error instead of a silent reset — overwriting a
  corrupt file would have destroyed the only copy of the stored tokens.
- LinkedIn's commentary and title limits are counted in **characters**, not
  bytes. A 3000-character post full of accents or emoji was previously rejected.
- LinkedIn calls now go through the shared pooled HTTP client, with connect and
  request timeouts, instead of building a client per request.
- Percent-encoding of the LinkedIn authorization URL is complete, rather than a
  hand-rolled substitution of six characters.
- `status` reports LinkedIn expiry dates alongside the remaining days.

### Fixed

- **Publishing calls are never repeated after a 5xx or a timeout.** Only
  failures that provably did not reach the platform — a refused connection, or
  an explicit "throttled, not processed" — are retried on a write. A retry on a
  timed-out publish could have posted twice.
- A concurrent `reshare` (and now `broadcast`) refreshes its tokens up front,
  sequentially. Two tasks renewing at the same time each rewrote the whole
  config file from its own stale copy, losing the other's new token.
- Downloading a remote `--image` no longer decodes the bytes as UTF-8, which
  corrupted every non-ASCII byte of the image.

### Dependencies

- Bumped: `rand` 0.8 → 0.9 (migrated to the new `rng()` / `random()` API),
  `tokio` 1.52.3, `clap` 4.6.1, `serde_json` 1.0.150, `open` 5.3.6.
- Added, behind the `meta` feature: `hmac` 0.12, `sha2` 0.10 (for
  `appsecret_proof`), and `reqwest/multipart`.
- Pinned toolchain via `rust-toolchain.toml` so local and CI use the same Rust /
  clippy version.
- `scripts/check.sh` runs the exact CI checks locally in one command; the opt-in
  `.githooks/pre-push` hook blocks a push when they fail
  (`git config core.hooksPath .githooks`).
- CI: bumped `actions/checkout` to v7 and installs the pinned toolchain via
  `rustup show`.

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

[Unreleased]: https://github.com/z29k/rustedin/compare/v2.0.0...HEAD
[2.0.0]: https://github.com/z29k/rustedin/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/z29k/rustedin/releases/tag/v1.0.0
