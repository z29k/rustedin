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

rustedin stores OAuth tokens and LinkedIn app **client secrets** in a local
`rustedin.json` file next to the binary (or at the path given via `--config`).

- This file is **git-ignored** and must **never** be committed or shared.
- If a `rustedin.json` (or any client secret / access token) is ever exposed,
  **rotate the affected LinkedIn app client secret** in the
  [LinkedIn Developer portal](https://www.linkedin.com/developers/apps) and
  re-authenticate every account with `rustedin auth`.

## Supported versions

Only the latest released version receives security fixes.
