# Security Policy

## How Curier Handles Your Data

Curier is a local-first email client. Here's how it works:

- **Credentials** (OAuth tokens, Client ID, Client Secret) are stored in the **macOS Keychain**, not in plain text files or the app's data directory.
- **Email data** is cached locally in a **SQLite database** on your machine for offline access and performance.
- **No telemetry.** Curier does not send any data to third-party servers. All communication is directly between your machine and your email provider's API (Gmail, Outlook).
- **HTML emails** are sanitized using the `ammonia` library before rendering to prevent XSS and other injection attacks.

## OAuth Scopes

Curier requests the `mail.google.com` scope, which grants full access to your Gmail account (read, compose, send, and delete). This scope is required to support Curier's full feature set, including composing and sending emails.

We recognize this is a broad permission. Curier only performs actions you explicitly initiate — it never sends, deletes, or modifies emails in the background without your direct action.

## Reporting a Vulnerability

If you discover a security vulnerability in Curier, **please report it privately** so we can address it before it's publicly disclosed.

**How to report:**

1. **Preferred:** Use GitHub's private vulnerability reporting — go to the [Security tab](https://github.com/yourusername/curier/security) on the repository and click **Report a vulnerability**
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
