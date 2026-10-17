# Curier - Quick Start Guide

## ⚙️ Prerequisites

### Enable Gmail API (One-time setup)
1. Visit [Google Cloud Console](https://console.cloud.google.com/apis/library/gmail.googleapis.com)
2. Click **Enable** for Gmail API
3. Wait 1-2 minutes for changes to propagate

### Google OAuth Credentials
- OAuth Client Type: **Desktop application**
- Redirect URI: `http://127.0.0.1` (Desktop clients accept any loopback port; Curier picks a free port per login)
- Required Scopes:
  - `https://www.googleapis.com/auth/gmail.readonly`
  - `https://www.googleapis.com/auth/gmail.send`
  - `email`

## 🚀 Start the App

```bash
cd curier
export PATH="$HOME/.cargo/bin:$PATH"
npm run tauri dev
```

## 🔧 Common Commands

### Clear Cached Credentials
```bash
security delete-generic-password -s "com.curier.credentials" -a "accounts"
```

### Watch Backend Logs
```bash
npm run tauri dev 2>&1 | grep -E "(🔑|📦|🔐|Error)"
```

## 🎯 Using the App

1. **Login:** Enter Google OAuth credentials and click "Connect Gmail"
2. **Authorize:** Complete OAuth flow in browser
3. **View Emails:** See 20 most recent emails in inbox
4. **Read Email:** Click any email to view full content
5. **Back to Inbox:** Click back button to return to list
6. **Refresh:** Click refresh button to fetch latest emails
7. **Logout:** Click logout to sign out

## 📝 Important Files

- `src/App.tsx` - Main UI (simplified, inline state)
- `src-tauri/src/commands/auth.rs` - OAuth commands
- `src-tauri/src/services/oauth.rs` - OAuth service
- `src-tauri/src/state.rs` - In-memory caching (prevents multiple Keychain prompts)

## 🐛 If Something Breaks

1. **Blank screen:** Check console for React errors
2. **Can't login:** the browser consent must finish within 5 minutes; close the tab and retry from Curier
3. **Multiple password prompts:** Check AppState caching in `state.rs`
4. **App won't start:** Check if cargo is in PATH

## 📖 Full Details

See `docs/GMAIL_SETUP.md` for the full Google Cloud setup walkthrough and `docs/TROUBLESHOOTING.md` for common problems.
