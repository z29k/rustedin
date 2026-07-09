# Contributing to rustedin

Thanks for taking the time to contribute! This document explains the branching
model, how to set up your environment, and the process for opening a pull
request.

## Code of Conduct

By participating in this project you agree to abide by our
[Code of Conduct](CODE_OF_CONDUCT.md).

## Branching model

rustedin follows a **Git Flow**-style model with two long-lived branches:

| Branch | Role | Deploys to |
|--------|------|------------|
| `main` | Production-ready code. Every commit is a released (or releasable) state. | **Releases** (`vX.Y.Z`) |
| `develop` | Integration branch. Where finished work lands before a release. | **Pre-releases** (`vX.Y.Z-rc.N`) |

Short-lived branches:

| Prefix | Purpose | Branch from | Merge into |
|--------|---------|-------------|------------|
| `feature/*` | New functionality | `develop` | `develop` |
| `fix/*` | Bug fixes | `develop` | `develop` |
| `hotfix/*` | Urgent production fix | `main` | `main` **and** `develop` |

```
feature/xyz ─┐
fix/abc ─────┼──► develop ──► main ──► tag vX.Y.Z ──► Release
hotfix/urgent ──────────────► main ──► tag vX.Y.Z
```

**All pull requests target `develop`** (except `hotfix/*`, which targets `main`).

## Development setup

Requires [Rust](https://rustup.rs/) (edition 2021, 1.74+).

```bash
git clone git@github.com:z29k/rustedin.git
cd rustedin
cargo build
```

Before pushing, make sure the same checks CI runs pass locally:

```bash
cargo fmt --all --check                       # formatting
cargo clippy --all-targets -- -D warnings     # linting (warnings are errors)
cargo build --release                         # it compiles
cargo test                                    # tests (if any)
```

> **Never commit `rustedin.json`.** It holds OAuth tokens and client secrets and
> is already listed in `.gitignore`. See [SECURITY.md](SECURITY.md).

## Commit messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<optional scope>): <short summary>
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`.

Examples:

```
feat(share): support full-size image mode
fix(auth): handle missing refresh_token in token response
docs(readme): document the comments command
```

## Pull request checklist

1. Branch off `develop` with a `feature/*` or `fix/*` name.
2. Keep the change focused; one logical change per PR.
3. Ensure `fmt`, `clippy`, and `build` all pass.
4. Update the docs (`README.md` / `README.fr.md`) if behavior changed.
5. Add an entry to the **Unreleased** section of [CHANGELOG.md](CHANGELOG.md).
6. Open the PR against `develop` and fill in the PR template.

## Release process (maintainers)

1. Merge everything intended for the release into `develop`.
2. Cut a pre-release for validation: tag `develop` with `vX.Y.Z-rc.1` and push
   the tag → the release workflow builds binaries and publishes a **pre-release**.
3. When validated, open a PR `develop` → `main` and merge.
4. Update the version in `Cargo.toml` and move the CHANGELOG `Unreleased`
   entries under the new version.
5. Tag `main` with `vX.Y.Z` and push the tag → the release workflow builds
   binaries and publishes a **release**.

Tags drive releases:

| Tag | Result |
|-----|--------|
| `vX.Y.Z` | Full release |
| `vX.Y.Z-rc.N`, `-beta.N`, `-alpha.N` | Pre-release |
