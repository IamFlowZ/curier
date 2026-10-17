# Security Policy

## How Curier Handles Your Data

Curier is a local-first email client. Here's how it works:

- **Credentials** (OAuth tokens, Client ID, Client Secret) are stored in the **macOS Keychain**, not in plain text files or the app's data directory.
- **Email data** (subjects, senders, snippets and full HTML/text bodies) is cached locally in a **SQLite database** on your machine for offline access and performance. The database is **not encrypted**; it is protected only by filesystem permissions on your user account (on macOS it lives under `~/Library/Application Support/`, which is private to your user). Anyone with access to your user account or an unencrypted disk image can read it, so use FileVault (or your OS's full-disk encryption) if that matters to you.
- **Settings** (sync interval, approved image domains, tab order) are written to a file with an `.enc` suffix. That encryption is keyed from values any process running as your user can derive, so treat it as obfuscation, not a security control. The settings file contains no credentials.
- **No telemetry.** Curier does not send any data to third-party servers. All communication is directly between your machine and your email provider's API (Gmail, Outlook).
- **HTML emails** are sanitized using the `ammonia` library before rendering to prevent XSS and other injection attacks.

## OAuth Scopes

Curier requests only the Gmail scopes it uses:

| Scope | Why |
|---|---|
| `https://www.googleapis.com/auth/gmail.readonly` | Listing and reading messages and attachments |
| `https://www.googleapis.com/auth/gmail.send` | Sending replies and new messages |
| `email` | Learning which address was authorized so the account can be labelled |

Curier **does not** request the full-mailbox `https://mail.google.com/` scope and cannot modify, label, trash or delete messages in your account. If you previously authorized Curier with the broader scope, revoke it at [myaccount.google.com/permissions](https://myaccount.google.com/permissions) and sign in again.

Curier only performs actions you explicitly initiate — it never sends emails in the background without your direct action.

## Reporting a Vulnerability

If you discover a security vulnerability in Curier, **please report it privately** so we can address it before it's publicly disclosed.

**How to report:**

1. **Preferred:** Use GitHub's private vulnerability reporting — go to the [Security tab](https://github.com/IamFlowZ/curier/security) on the repository and click **Report a vulnerability**
2. **Alternative:** Email **dakota.lewallen17@gmail.com** with the subject line `[Curier Security]`

**Please include:**
- A description of the vulnerability
- Steps to reproduce it
- The potential impact (what an attacker could do)
- Any suggested fixes, if you have them

**What to expect:**
- Acknowledgment within 48 hours
- A fix or mitigation plan within 7 days for critical issues
- Credit in the release notes (unless you prefer to remain anonymous)

**Please do not:**
- Open a public GitHub issue for security vulnerabilities
- Share details publicly before a fix is available
- Exploit the vulnerability against other users

## Supported Versions

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | Best effort |

## Security Best Practices for Users

- **Keep Curier updated** to get the latest security fixes
- **Don't share your Client ID and Client Secret** publicly (e.g., in screenshots or GitHub issues)
- **Review the OAuth consent screen** when authorizing — make sure you're granting access to your own Google Cloud project, not someone else's
- **Revoke access** if you stop using Curier: go to [myaccount.google.com/permissions](https://myaccount.google.com/permissions)
- **Only download Curier from the official repository** — forks may contain modified code
