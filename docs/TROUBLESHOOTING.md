# Troubleshooting

Common issues and solutions when setting up and using Curier.

---

## OAuth & Authentication

### "This app isn't verified" warning in browser

**Expected behavior.** Your Google Cloud project is in testing mode, so Google shows this warning. Click **Advanced** → **Go to [your app name] (unsafe)** to continue. This is safe — you created the app and you're the only user.

### "Access blocked: This app's request is invalid" (Error 400)

Your OAuth consent screen may not be configured correctly, or the Gmail address isn't added as a test user.

**Fix:**
1. Go to [Google Cloud Console](https://console.cloud.google.com/) → **APIs & Services** → **OAuth consent screen**
2. Make sure the app is set to **External** and has the required fields filled in
3. Under **Test users**, add the Gmail address you're trying to connect
4. Try again in Curier

### "Address already in use" (Error 48)

Port 8080 is in use by another process. Curier needs this port for the OAuth callback.

**Fix:**
```bash
lsof -ti:8080 | xargs kill -9
```

Then try connecting again.

### "CSRF token mismatch"

This happens when multiple login attempts overlap.

**Fix:** Quit Curier completely and relaunch it, then try connecting once.

### Authentication works but expires after 7 days

Google tokens for apps in "Testing" mode expire after 7 days. This is a Google-imposed limit for unverified apps.

**Fix:** When this happens, Curier will prompt you to re-authenticate. This is a known limitation of the testing mode. The token refresh should handle most cases automatically, but if it doesn't, just sign in again.

---

## Email Display

### Emails not loading / empty inbox

1. Make sure the Gmail API is enabled in your Google Cloud project
2. Check that your OAuth scopes include `gmail.readonly`
3. Try clicking **Refresh** in Curier
4. If the issue persists, try removing and re-adding the account

### HTML emails look broken

Curier sanitizes HTML emails for security using the `ammonia` library. Some complex email layouts (heavy CSS, embedded forms) may not render perfectly. Plain text fallback is available for emails that don't render well.

---

## Performance

### High memory usage

Curier is designed to stay under 150MB of RAM. If you notice higher usage:

1. Check the memory monitor in the bottom of the app
2. Try refreshing the email list
3. Restart the app if usage is consistently above 150MB

Please open a GitHub issue if you consistently see high memory usage — include the number shown in the memory monitor.

---

## macOS Specific

### Keychain password prompt on every launch

Curier stores your OAuth credentials in the macOS Keychain. If you're being prompted for your password repeatedly:

1. When the Keychain prompt appears, select **Always Allow**
2. If that doesn't work, try removing the Curier keychain entry and re-authenticating:
   ```bash
   security delete-generic-password -s "curier" -a "gmail_credentials"
   ```

### App won't open — "damaged" or "unidentified developer" warning

If you downloaded a pre-built binary:
```bash
xattr -cr /Applications/Curier.app
```

Or right-click the app → **Open** → **Open** to bypass Gatekeeper.

---

## Build Issues

### `npm run tauri dev` fails

Make sure you have the prerequisites:

```bash
# Check Node.js version (need 20.19+ or 22.12+)
node --version

# Check Rust is installed
rustc --version

# Install frontend dependencies
npm install
```

### Rust compilation errors

```bash
# Update Rust toolchain
rustup update

# Clean and rebuild
cd src-tauri && cargo clean && cd ..
npm run tauri dev
```

---

## Still Stuck?

1. Run the setup verification script: `./scripts/verify-gmail-setup.sh`
2. Check [GitHub Issues](https://github.com/yourusername/curier/issues) — someone may have hit the same problem
3. Open a new issue with:
   - Your macOS version
   - The error message (screenshot or text)
   - Steps to reproduce
