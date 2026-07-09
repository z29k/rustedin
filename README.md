<p align="center">
  <img src="assets/rustedin-logo.jpg" alt="rustedin logo" width="320">
</p>

<h1 align="center">rustedin</h1>

<p align="center">
  Multi-account LinkedIn CLI — post, share and reshare from personal accounts and company pages.
</p>

<p align="center">
  <a href="https://github.com/z29k/rustedin/actions/workflows/ci.yml"><img src="https://github.com/z29k/rustedin/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/z29k/rustedin/releases"><img src="https://img.shields.io/github/v/release/z29k/rustedin?include_prereleases&sort=semver" alt="Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.74%2B-orange.svg" alt="Rust 1.74+">
</p>

<p align="center">
  <b>English</b> · <a href="README.fr.md">Français</a>
</p>

---

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Quick start](#quick-start)
- [Command reference](#command-reference)
  - [Global `--config` option](#global---config-option)
  - [`setup`](#setup)
  - [`auth`](#auth)
  - [`accounts`](#accounts)
  - [`status`](#status)
  - [`post`](#post)
  - [`reshare`](#reshare)
  - [`share`](#share)
  - [`get-post`](#get-post)
  - [`comments`](#comments)
  - [`profile`](#profile)
- [Visibility (`--visibility`)](#visibility---visibility)
- [Configuration file (`rustedin.json`)](#configuration-file-rustedinjson)
- [Token management](#token-management)
- [Limitations](#limitations)
- [Common errors](#common-errors)
- [Exit codes](#exit-codes)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

---

## Features

- **Multi-account** — manage as many personal accounts and company pages as you want, each behind a short alias.
- **Text posts** — publish a post with configurable audience.
- **Link sharing** — share an external article with a link-preview card or a full-size image, optional thumbnail from a local file or URL.
- **Fan-out reshares** — reshare a post from several accounts at once, concurrently.
- **Read helpers** — fetch a post, its comments, or a member's profile.
- **Automatic token refresh** — access tokens are refreshed transparently before they expire.
- **Single self-contained binary** — no runtime dependencies, config stored in one portable JSON file.

---

## Installation

### Prebuilt binary

Download the binary for your platform from the [Releases](https://github.com/z29k/rustedin/releases) page and place it somewhere on your `PATH`.

### From source

Requires [Rust](https://rustup.rs/) (edition 2021, 1.74+).

```bash
# Optimized release build → target/release/rustedin
cargo build --release

# Or install straight into ~/.cargo/bin (available everywhere as `rustedin`)
cargo install --path .
```

---

## Quick start

### Step 1 — Create the LinkedIn apps

LinkedIn requires **two separate apps** (platform restriction: the "Community Management API" must be the *only* active product on its app).

**App for personal accounts**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App**
2. Enable the products:
   - **Share on LinkedIn**
   - **Sign In with LinkedIn using OpenID Connect**
3. **Auth** tab → add `http://localhost:8765/callback` as a **Redirect URL**
4. Copy the **Client ID** and **Client Secret**

**App for company pages**

1. [linkedin.com/developers/apps](https://www.linkedin.com/developers/apps) → **Create App** (a second app)
2. Enable the product:
   - **Community Management API**
3. **Auth** tab → add `http://localhost:8765/callback` as a **Redirect URL**
4. Copy the **Client ID** and **Client Secret**

### Step 2 — Configure the apps

```bash
rustedin setup --app=personal     --client-id=PERSONAL_CLIENT_ID --client-secret=PERSONAL_CLIENT_SECRET
rustedin setup --app=organization --client-id=ORG_CLIENT_ID      --client-secret=ORG_CLIENT_SECRET
```

### Step 3 — Authenticate each account

```bash
# Personal account
rustedin auth --account=quentin

# Company page (the page admin must log in)
rustedin auth --account=my-company --org-id=YOUR_ORG_ID
```

> The **org-id** is found in the company page URL: `linkedin.com/company/MY_ORG_ID/`
>
> For a company account, the person who logs in **must be an admin of the page**.

### First post

```bash
rustedin post --account=quentin --text="My first post via rustedin!"
```

---

## Command reference

### Global `--config` option

Path to the `rustedin.json` configuration file.

```
rustedin --config <PATH> <command> [options]
```

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--config` | no | `rustedin.json` next to the binary | Absolute or relative path to the configuration file |

> The `--config` option must appear **before** the command name.

```bash
rustedin --config /etc/rustedin.json accounts
```

---

### `setup`

Configure the credentials of a LinkedIn app. Run before `auth`.

```
rustedin setup --app=<TYPE> --client-id=<ID> --client-secret=<SECRET>
```

| Option | Required | Description |
|--------|----------|-------------|
| `--app` | yes | App type: `"personal"` or `"organization"` |
| `--client-id` | yes | Client ID of the LinkedIn app |
| `--client-secret` | yes | Client Secret of the LinkedIn app |

**Behavior:** partial update — only the credentials for the given app type are changed. Re-run to update.

**Output:** nothing on stdout. On stderr:

```
App personal credentials saved to /path/to/rustedin.json
```

---

### `auth`

Authenticate a LinkedIn account via OAuth 2.0. Opens the browser automatically.

```
rustedin auth --account=<NAME> [--org-id=<ID>]
```

| Option | Required | Description |
|--------|----------|-------------|
| `--account` | yes | Account alias (free-form name, e.g. `"quentin"`, `"my-company"`) |
| `--org-id` | no | LinkedIn organization ID (company pages only) |

**Account type detection:**

- `--org-id` present → type `organization` (uses the organization app credentials)
- otherwise → type `person` (uses the personal app credentials)

**Requested OAuth scopes:**

| Type | Scopes |
|------|--------|
| `person` | `w_member_social openid profile email` |
| `organization` | `r_organization_social w_organization_social` |

> **Why not `openid profile email` for an org?** LinkedIn requires the **Community Management API** to be the *only* product on its app. The `openid profile email` scopes (from the *Sign In with LinkedIn using OpenID Connect* product) therefore cannot be added — requesting them would raise `unauthorized_scope_error`.

**Flow:**

1. Opens the browser to the LinkedIn authorization page (or prints the URL if the browser doesn't open)
2. Listens on `http://localhost:8765/callback` (port 8765)
3. Timeout: **5 minutes** — after that, the command fails
4. Exchanges the authorization code for tokens
5. **Personal accounts only:** fetches the user profile (`/v2/userinfo`) to resolve the `person_urn`. For an org, the URN (`urn:li:organization:<org-id>`) is derived directly from `--org-id`
6. Stores the tokens in `rustedin.json`

**Output:** nothing on stdout. On stderr:

```
Account "quentin" saved to /path/to/rustedin.json
  Type          : person
  URN           : urn:li:person:abc123
  Authorized by : Quentin Dupont (urn:li:person:abc123)
  Access token  : 60 days (auto-refreshed)
  Refresh token : 365 days (rustedin auth once/year)
```

---

### `accounts`

List all configured accounts with their state.

```
rustedin accounts
```

**Output (stdout):** JSON array.

```json
[
  {
    "alias": "quentin",
    "type": "person",
    "urn": "urn:li:person:abc123",
    "access_token_status": "active",
    "access_token_expires_in_days": 45,
    "refresh_token_expires_in_days": 320
  }
]
```

**If no account is configured:** nothing on stdout. On stderr: `No accounts configured. Run: rustedin auth --account=<alias>`

---

### `status`

Show detailed token state for each account.

```
rustedin status
```

**Output (stdout):** JSON **object** (key = account alias).

> Unlike `accounts` (which returns an array), `status` returns an object indexed by alias, and adds the `needs_reauth` field.

```json
{
  "quentin": {
    "type": "person",
    "urn": "urn:li:person:abc123",
    "access_token_status": "active",
    "access_token_expires_in_days": 45,
    "refresh_token_expires_in_days": 320,
    "needs_reauth": false
  }
}
```

---

### `post`

Publish a text post.

```
rustedin post --account=<NAME> --text=<CONTENT> [--visibility=<LEVEL>]
```

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--account` | yes | — | Account alias |
| `--text` | yes | — | Post content (1–3000 characters) |
| `--visibility` | no | `PUBLIC` | Audience (see [Visibility](#visibility---visibility)) |

**Output (stdout):** JSON with `success`, `post_id`, `account`, `urn`, `visibility`, `text_preview` (first 100 chars).

```bash
rustedin post --account=quentin --text="Hello LinkedIn!"
rustedin post --account=quentin --text="Visible to my connections" --visibility=connections
```

---

### `reshare`

Reshare an existing post from one or several accounts. Reshares run concurrently.

```
rustedin reshare --post-id=<URN> --accounts=<LIST> [--commentary=<TEXT>] [--visibility=<LEVEL>]
```

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--post-id` | yes | — | URN of the post to reshare (e.g. `"urn:li:share:7123456789"`) |
| `--accounts` | yes | — | Comma-separated aliases, or `"*"` |
| `--commentary` | no | `""` | Comment above the reshare |
| `--visibility` | no | `PUBLIC` | Audience |

**Wildcard `"*"`:** expands to all `person` accounts only. `organization` accounts are ignored.

**Concurrent execution:** all reshares run in parallel. A failure on one account does not prevent the others.

**Output (stdout):** JSON with `total`, `succeeded`, `failed`, and a per-account `results` array.

```bash
# Reshare from all personal accounts
rustedin reshare --post-id="urn:li:share:7123456789" --accounts="*"

# Reshare from specific accounts with a comment
rustedin reshare --post-id="urn:li:share:7123456789" --accounts=quentin,alice --commentary="Worth a read!"
```

---

### `share`

Share an external article or URL with a link preview. Optionally attach an image (thumbnail) from a local path or URL.

```
rustedin share --account=<NAME> --url=<URL> --title=<TITLE> \
  [--description=<DESC>] [--commentary=<TEXT>] [--visibility=<LEVEL>] \
  [--image=<PATH_OR_URL>] [--mode=<MODE>]
```

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--account` | yes | — | Account alias |
| `--url` | yes | — | URL of the article to share |
| `--title` | yes | — | Article title (1–400 characters) |
| `--description` | no | — | Article description. **Note:** LinkedIn does not display it in the feed link preview; if `--commentary` is not provided, the description is used as the comment above the link |
| `--commentary` | no | `""` | Personal comment above the link |
| `--visibility` | no | `PUBLIC` | Audience |
| `--image` | no | — | Article image: local path or URL (max 10 MB). Used as thumbnail in `article` mode, or full-size image in `image` mode |
| `--mode` | no | `article` | `article` (link preview card, default) or `image` (full image + link in text). In `image` mode, `--image` is required |

**Image detection:** if the value starts with `http://` or `https://` it is treated as a URL (downloaded, then uploaded); otherwise it is treated as a local file path.

**Output (stdout):** JSON with `success`, `post_id`, `account`, `urn`, `url`, `mode`, and `image_urn` (only when `--image` was used).

```bash
# Simple share (no image)
rustedin share --account=quentin \
  --url="https://example.com/article" \
  --title="A great article" \
  --commentary="I recommend this read!"

# Full-size image mode
rustedin share --account=quentin \
  --url="https://example.com/article" \
  --title="A great article" \
  --image="https://example.com/photo.jpg" \
  --mode=image
```

---

### `get-post`

Retrieve a post by its URN (useful to inspect what LinkedIn stored).

```
rustedin get-post --account=<NAME> --post-id=<URN>
```

| Option | Required | Description |
|--------|----------|-------------|
| `--account` | yes | Account alias (for authentication) |
| `--post-id` | yes | Post URN (e.g. `"urn:li:share:123456"`) |

**Output (stdout):** the raw LinkedIn API JSON for the post.

---

### `comments`

Get the comments on a LinkedIn post.

```
rustedin comments --account=<NAME> --post-id=<URN> [--count=<N>] [--start=<N>]
```

| Option | Required | Default | Description |
|--------|----------|---------|-------------|
| `--account` | yes | — | Account alias (for authentication) |
| `--post-id` | yes | — | Post URN (`urn:li:share:xxx`, `urn:li:activity:xxx` or `urn:li:ugcPost:xxx`) |
| `--count` | no | `20` | Number of comments to fetch (1–100) |
| `--start` | no | `0` | Pagination start index |

**Output (stdout):** JSON with `post_id`, `account`, `start`, `count`, `total`, and a `comments` array.

---

### `profile`

Get profile info of a member. With no `--urn`, returns your own profile (personal accounts only).

```
rustedin profile --account=<NAME> [--urn=<PERSON_URN>]
```

| Option | Required | Description |
|--------|----------|-------------|
| `--account` | yes | Account alias (for authentication) |
| `--urn` | no | URN of another member to look up (e.g. `urn:li:person:xxx`); omit for your own profile |

> Own-profile lookup is not available for `organization` accounts (no `openid` scope). Pass a `--urn` instead.

---

## Visibility (`--visibility`)

Available on `post`, `reshare` and `share`. Controls who can see the post.

| Value | Description |
|-------|-------------|
| `PUBLIC` | Visible to everyone **(default)** |
| `CONNECTIONS` | Visible only to 1st-degree connections |
| `LOGGED_IN` | Visible only to logged-in LinkedIn members |

> The value is **case-insensitive**: `connections`, `Connections` and `CONNECTIONS` are equivalent.

---

## Configuration file (`rustedin.json`)

### Location

| Method | Path |
|--------|------|
| Default | `rustedin.json` next to the binary (portable) |
| Override | `rustedin --config /path/to/rustedin.json <command>` |

The file is created automatically if it doesn't exist.

### Schema

```json
{
  "linkedInApp": {
    "personal":     { "client_id": "...", "client_secret": "..." },
    "organization": { "client_id": "...", "client_secret": "..." }
  },
  "accounts": {
    "<alias>": {
      "alias": "string",
      "type": "person | organization",
      "urn": "string | null",
      "person_urn": "string | null",
      "access_token": "string",
      "refresh_token": "string",
      "expires_at": "number (Unix ms)",
      "refresh_expires_at": "number (Unix ms)"
    }
  }
}
```

> ⚠️ **Security:** this file contains OAuth tokens and client secrets. **Never share or commit it.** It is already listed in `.gitignore`.

---

## Token management

| Token | Lifetime | Refresh |
|-------|----------|---------|
| Access token | 60 days | Refreshed automatically **7 days before expiry**, transparently, on any API command |
| Refresh token | 365 days | Cannot be refreshed automatically — run `rustedin auth` once a year per account |

When a refresh happens, a message appears on stderr:

```
[rustedin] Refreshing token for "quentin"...
[rustedin] Token refreshed for "quentin".
```

| Situation | Symptom | Fix |
|-----------|---------|-----|
| Access token expired, refresh token valid | Automatic refresh | Nothing to do |
| Refresh token expired | `Refresh token for "X" has expired...` | `rustedin auth --account=X` |
| Refresh failed | `Token refresh failed for "X"...` | Check the network, then `rustedin auth --account=X` |

---

## Limitations

1. **Port 8765 required** — the OAuth callback listens on `127.0.0.1:8765` (hardcoded); the port must be free during `auth`.
2. **Text max 3000 characters** — text posts are limited to 3000 characters (client-side validation).
3. **Article title max 400 characters** — the `share` title is limited to 400 characters.
4. **No video upload** — only text posts, link shares (with optional image) and reshares are supported.
5. **No post deletion/editing** — once published, a post cannot be edited or deleted via rustedin.
6. **No client-side rate limiting** — if LinkedIn returns HTTP 429, the error is surfaced as-is; there is no automatic retry.
7. **No proxy support** — HTTP requests go directly to the network (no `HTTP_PROXY`, no CLI option).
8. **No environment variables** — all configuration goes through `rustedin.json` or the `--config` flag.
9. **Wildcard `*` = person accounts only** — in `reshare --accounts="*"`, only `person` accounts are targeted.
10. **Fixed redirect URI** — the OAuth callback URL is hardcoded to `http://localhost:8765/callback` and must be registered as-is in each LinkedIn app.
11. **Fixed LinkedIn API version** — pinned to `202603` (the `LinkedIn-Version` header).
12. **No HTTP request timeout** — LinkedIn API calls have no configured timeout (only the OAuth callback has a 5-minute timeout).
13. **Article description not displayed** — LinkedIn does not render the `description` field in feed link previews; if `--description` is given without `--commentary`, it is used as the comment above the link.
14. **LinkedIn reserved characters** — `( ) [ ] @ # * _ ~ { } < > | \` are reserved in LinkedIn's "little text" format. rustedin escapes them automatically with `\` to avoid silent text truncation.

---

## Common errors

| Message | Cause | Fix |
|---------|-------|-----|
| `App personal not configured. Run: rustedin setup ...` | Missing credentials | `rustedin setup --app=personal ...` |
| `Invalid app type "X". Use "personal" or "organization".` | Wrong `--app` value | Use `"personal"` or `"organization"` |
| `Account "X" not found. Available: ...` | Unknown alias | Check the alias or run `rustedin auth --account=X` |
| `No URN stored for "X". Run: rustedin auth --account=X` | Interrupted/incomplete auth | Re-run `rustedin auth --account=X` |
| `Refresh token for "X" has expired...` | Refresh token > 365 days | `rustedin auth --account=X` |
| `Failed to bind port 8765: ...` | Port already in use | Free port 8765 (`lsof -i :8765`) |
| `Timeout waiting for OAuth callback (5 min)` | No browser response | Re-run and open the URL manually |
| `Auth failed: unauthorized_scope_error` | App missing required product. **Personal:** enable *Share on LinkedIn* + *Sign In with LinkedIn using OpenID Connect*. **Org:** enable *Community Management API* (only product allowed) | Add the product in the app's *Products* tab, then re-run `rustedin auth` |
| `Text must be between 1 and 3000 characters` | Empty or too-long text | Adjust the text length |
| `Title must be between 1 and 400 characters` | Empty or too-long title | Adjust the title length |
| `LinkedIn API error STATUS for "X": ...` | LinkedIn rejected the request | Read the error body for details |
| `Image too large (X.X MB). LinkedIn allows max 10 MB.` | Image > 10 MB | Reduce the image size |

---

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Error (the message is printed on stderr) |

---

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model (`main` / `develop`, `feature/*` and `fix/*` → PR into `develop`), commit conventions, and the release process. By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

---

## Security

Found a vulnerability? Please follow the process in [SECURITY.md](SECURITY.md) — do not open a public issue for security reports.

---

## License

Released under the [MIT License](LICENSE). © 2026 z29k.
