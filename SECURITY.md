# Security Policy

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, use one of the following private channels:

- Open a [private security advisory](https://github.com/z29k/rustedin/security/advisories/new) on GitHub, **or**
- Email **security@z29k.fr** with the details.

Please include:

- A description of the vulnerability and its impact.
- Steps to reproduce (a minimal proof of concept if possible).
- Any suggested remediation.

We will acknowledge your report as quickly as possible and keep you informed
about the fix and disclosure timeline.

## Handling of secrets

rustedin stores OAuth tokens, Facebook Page tokens and the **client secrets**
of every configured app in a single local `rustedin.json` file next to the
binary (or at the path given via `--config`). It is written `0600` on Unix.

- This file is **git-ignored** and must **never** be committed or shared.
- If a `rustedin.json` (or any client secret / access token) is ever exposed,
  rotate the affected app secret and re-authenticate every account:
  - **LinkedIn** — [LinkedIn Developer portal](https://www.linkedin.com/developers/apps),
    then `rustedin linkedin auth --account=<alias>`.
  - **Meta** — [Meta App Dashboard](https://developers.facebook.com/apps) →
    Settings → Basic → App Secret, then `rustedin meta auth --account=<alias>`.
    Rotating the app secret also invalidates every Page token derived from it.

## Supported versions

Only the latest released version receives security fixes.
