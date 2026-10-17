<p align="center">
  <img src="logo.svg" alt="Curier" width="280" />
</p>

<p align="center">
  A fast, lightweight, open-source email client built with Tauri and Rust.
</p>

<p align="center">
  <strong>~12MB RAM</strong> &nbsp;·&nbsp; Multi-account &nbsp;·&nbsp; Modern UI &nbsp;·&nbsp; Native performance
</p>

---

## Why Curier?

Most modern email clients are built on Electron and consume 300–500MB of RAM just to show your inbox. Native clients like Apple Mail are lighter, but locked to one platform with limited flexibility.

Curier takes a different approach: a **Rust backend** for speed and safety, a **React frontend** for a modern UI, and **Tauri** to bridge them — all in a package that uses a fraction of the memory.

| Client | RAM Usage | Open Source |
|---|---|---|
| Mailspring | 300–500MB | Partial |
| Thunderbird | 200–400MB | Yes |
| Apple Mail | ~100MB | No |
| **Curier** | **~12MB** | **Yes** |

## Features

- **Lightweight** — runs at ~12MB RAM, well under the 150MB ceiling it was designed for
- **Multi-account** — manage multiple Gmail accounts from a single window
- **Tabbed interface** — open emails, compose windows, and account views in tabs
- **Feeds view** — browse newsletters and subscriptions in a dedicated feed layout
- **Fast search** — search across your inbox quickly
- **Dark theme** — easy on the eyes, built-in
- **Secure auth** — OAuth2 PKCE flow with credentials stored in the macOS Keychain
- **HTML email rendering** — sanitized with `ammonia` for safe viewing
- **SQLite storage** — local email cache for fast offline browsing

## Screenshots

<p align="center">
  <img src="docs/screenshots/inbox.png" alt="Inbox view with multi-account support" width="700" />
  <br />
  <em>Multi-account inbox with tabbed navigation</em>
</p>

<p align="center">
  <img src="docs/screenshots/feeds.png" alt="Feeds view" width="700" />
  <br />
  <em>Newsletter feeds view</em>
</p>

<p align="center">
  <img src="docs/screenshots/compose.png" alt="Compose window" width="700" />
  <br />
  <em>Compose a new email</em>
</p>

## Tech Stack

- **Backend:** Rust (Tauri 2.x)
- **Frontend:** React 19, TypeScript, Tailwind CSS, Vite
- **Email:** Gmail REST API with OAuth2 PKCE
- **Storage:** SQLite via `rusqlite`
- **Security:** HTML sanitization via `ammonia`, credentials in macOS Keychain

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) 20.19+ or 22.12+
- [Rust](https://rustup.rs/) (via `rustup`)
- A Google Cloud project with Gmail API enabled and OAuth2 Desktop credentials

### Setup

```bash
# Clone the repo
git clone https://github.com/yourusername/curier.git
cd curier

# Install frontend dependencies
npm install

# Run the development build
npm run tauri dev
```

On first launch, you'll be prompted to enter your Google OAuth Client ID, Client Secret, and Gmail address. Curier will open your browser to complete the Google authorization flow.

### Google Cloud Setup

1. Go to the [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project (or use an existing one)
3. Enable the **Gmail API**
4. Go to **Credentials** → **Create Credentials** → **OAuth 2.0 Client ID**
5. Select **Desktop application** as the type
6. Copy the Client ID and Client Secret into Curier's login screen

### Building for Production

```bash
npm run tauri build
```

This produces a native macOS `.dmg` in `src-tauri/target/release/bundle/`.

## Architecture

```
curier/
├── src/                    # React frontend
│   ├── components/         # UI components
│   ├── features/           # Feature modules (auth, email, feeds, tabs, etc.)
│   ├── hooks/              # Custom React hooks
│   ├── store/              # Zustand state management
│   └── App.tsx             # Root component
├── src-tauri/              # Rust backend
│   └── src/
│       ├── commands/       # Tauri IPC commands (auth, email, system)
│       ├── models/         # Data models (auth tokens, email types)
│       ├── services/       # Core services (Gmail API, OAuth, SQLite, memory)
│       └── lib.rs          # App setup and command registration
```

## Roadmap

- [ ] Outlook / Microsoft 365 support (in progress)
- [ ] Linux builds
- [ ] Keyboard shortcuts
- [ ] Notifications
- [ ] Attachment handling improvements
- [ ] Contact autocomplete
- [ ] Email rules / filtering engine (in progress)
- [ ] Encrypted local storage
- [ ] Plugin system

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on setting up the development environment and submitting pull requests.

If you're looking for a place to start, check out issues labeled [`good first issue`](https://github.com/yourusername/curier/labels/good%20first%20issue).

## License

[MIT](LICENSE)

---

<p align="center">
  Built with <a href="https://v2.tauri.app">Tauri</a>, <a href="https://www.rust-lang.org">Rust</a>, and <a href="https://react.dev">React</a>.
</p>
