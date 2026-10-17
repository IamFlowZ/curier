# Gmail Setup Guide

Curier uses Gmail's API to access your email. Because Curier is an open-source project, each user creates their own Google Cloud credentials. This keeps your data private — Curier never touches a third-party server.

This guide walks you through the setup in about 5 minutes.

---

## Step 1: Create a Google Cloud Project

1. Go to [console.cloud.google.com](https://console.cloud.google.com/)
2. Sign in with your Google account
3. Click the project dropdown at the top of the page and select **New Project**
4. Name it something like `Curier Email` and click **Create**
5. Make sure the new project is selected in the dropdown

## Step 2: Enable the Gmail API

1. In the left sidebar, go to **APIs & Services** → **Library**
2. Search for **Gmail API**
3. Click on it, then click **Enable**

## Step 3: Configure the OAuth Consent Screen

1. Go to **APIs & Services** → **OAuth consent screen**
2. Select **External** as the user type, then click **Create**
3. Fill in the required fields:
   - **App name:** `Curier` (or anything you like)
   - **User support email:** your email
   - **Developer contact email:** your email
4. Click **Save and Continue**
5. On the **Scopes** page, click **Add or Remove Scopes** and add:
   - `https://www.googleapis.com/auth/gmail.readonly`
6. Click **Save and Continue**
7. On the **Test users** page, click **Add Users** and enter the Gmail address you want to use with Curier
8. Click **Save and Continue**, then **Back to Dashboard**

> **Note:** Your app will be in "Testing" mode, which means only the test users you add can use it. This is fine for personal use and avoids the need for Google's app verification process entirely.

## Step 4: Create OAuth Credentials

1. Go to **APIs & Services** → **Credentials**
2. Click **Create Credentials** → **OAuth 2.0 Client ID**
3. Select **Desktop application** as the application type
4. Name it `Curier Desktop` (or anything you like)
5. Click **Create**
6. You'll see a dialog with your **Client ID** and **Client Secret** — copy both

## Step 5: Enter Credentials in Curier

1. Open Curier
2. On the login screen, paste your **Client ID** and **Client Secret**
3. Enter the **Gmail address** you added as a test user
4. Click **Connect Gmail**
5. Your browser will open to Google's authorization page — sign in and grant access

That's it! Curier will now sync your email.

---

## Adding More Gmail Accounts

To add another Gmail account:

1. Go back to your Google Cloud project's **OAuth consent screen**
2. Add the new email address as a test user
3. In Curier, add a new account using the same Client ID and Client Secret, but with the new email address

## Keeping It Working

- Your Google Cloud project stays in "Testing" mode, which is free and doesn't expire
- Test users have a token that refreshes automatically — you shouldn't need to re-authenticate
- If you ever get an authentication error, try removing and re-adding the account

## Revoking Access

If you want to disconnect Curier from your Gmail:

1. Go to [myaccount.google.com/permissions](https://myaccount.google.com/permissions)
2. Find `Curier` (or whatever you named your app)
3. Click **Remove Access**

---

## FAQ

**Why do I need to create my own Google Cloud project?**
Google requires apps that access Gmail to go through a paid security review before they can be used publicly. Since Curier is a free, open-source project, we let each user create their own credentials instead. This also means your data flows directly between your computer and Google — no third-party servers involved.

**Is this safe?**
Yes. Your Client ID and Client Secret are stored locally in your macOS Keychain. The OAuth flow uses PKCE, which is the industry standard for desktop apps. Curier requests read-only access plus permission to send mail (`gmail.readonly` and `gmail.send`); it cannot modify or delete anything in your mailbox.

**What if I see a "This app isn't verified" warning?**
This is expected. Since your app is in testing mode, Google shows this warning. Click **Advanced** → **Go to Curier (unsafe)** to continue. Despite the scary language, this is safe — you created the app yourself and you're the only user.

**Can I use this with a Google Workspace (work) account?**
It depends on your organization's settings. Some Workspace admins restrict which apps can access Gmail. If you get an error, check with your IT admin.

**The 100-user limit — does it apply to me?**
No. The 100-user limit applies to apps in "Testing" mode that haven't been verified. Since you're both the developer and the user, this doesn't matter. You can add up to 100 test users to your project, which is more than enough for personal use.
