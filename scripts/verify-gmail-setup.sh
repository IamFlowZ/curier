#!/bin/bash
# verify-gmail-setup.sh
# Validates that your Google Cloud OAuth credentials are correctly configured
# for use with Curier before you try to connect.
#
# Usage: ./scripts/verify-gmail-setup.sh

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color
BOLD='\033[1m'

pass() { echo -e "  ${GREEN}✓${NC} $1"; }
fail() { echo -e "  ${RED}✗${NC} $1"; }
warn() { echo -e "  ${YELLOW}!${NC} $1"; }
info() { echo -e "  ${BLUE}→${NC} $1"; }

echo ""
echo -e "${BOLD}Curier — Gmail Setup Verification${NC}"
echo "─────────────────────────────────────"
echo ""

ERRORS=0

# ── Step 1: Check for required tools ──
echo -e "${BOLD}1. Checking prerequisites${NC}"

if command -v curl &> /dev/null; then
    pass "curl is installed"
else
    fail "curl is not installed (needed for API checks)"
    ERRORS=$((ERRORS + 1))
fi

# ── Step 2: Prompt for credentials ──
echo ""
echo -e "${BOLD}2. Enter your Google Cloud credentials${NC}"
echo ""

read -r -p "   Client ID: " CLIENT_ID
read -r -p "   Client Secret: " CLIENT_SECRET
read -r -p "   Gmail address: " EMAIL

echo ""

# ── Step 3: Validate Client ID format ──
echo -e "${BOLD}3. Validating credential format${NC}"

if [[ -z "$CLIENT_ID" ]]; then
    fail "Client ID is empty"
    ERRORS=$((ERRORS + 1))
elif [[ "$CLIENT_ID" == *".apps.googleusercontent.com" ]]; then
    pass "Client ID format looks correct"
else
    fail "Client ID should end with '.apps.googleusercontent.com'"
    info "Example: 123456789-abc123.apps.googleusercontent.com"
    ERRORS=$((ERRORS + 1))
fi

if [[ -z "$CLIENT_SECRET" ]]; then
    fail "Client Secret is empty"
    ERRORS=$((ERRORS + 1))
elif [[ ${#CLIENT_SECRET} -ge 10 ]]; then
    pass "Client Secret is present (${#CLIENT_SECRET} characters)"
else
    warn "Client Secret seems too short (${#CLIENT_SECRET} characters)"
fi

if [[ -z "$EMAIL" ]]; then
    fail "Email is empty"
    ERRORS=$((ERRORS + 1))
elif [[ "$EMAIL" == *"@gmail.com" ]] || [[ "$EMAIL" == *"@googlemail.com" ]]; then
    pass "Email is a Gmail address"
elif [[ "$EMAIL" == *"@"* ]]; then
    warn "Email appears to be a Workspace account — make sure your admin allows third-party app access"
else
    fail "Email doesn't look like a valid email address"
    ERRORS=$((ERRORS + 1))
fi

# ── Step 4: Test the OAuth endpoint ──
echo ""
echo -e "${BOLD}4. Testing Google OAuth endpoint${NC}"

RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" \
    "https://oauth2.googleapis.com/token" \
    -d "client_id=$CLIENT_ID" \
    -d "client_secret=$CLIENT_SECRET" \
    -d "grant_type=authorization_code" \
    -d "code=fake_code_for_validation" \
    -d "redirect_uri=http://127.0.0.1:8080" \
    2>/dev/null || echo "000")

if [[ "$RESPONSE" == "000" ]]; then
    fail "Could not reach Google's OAuth server — check your internet connection"
    ERRORS=$((ERRORS + 1))
elif [[ "$RESPONSE" == "400" || "$RESPONSE" == "401" ]]; then
    # 400/401 with a fake code is expected — it means Google recognized the client
    pass "Google OAuth endpoint is reachable and recognized your Client ID"
elif [[ "$RESPONSE" == "403" ]]; then
    fail "Google rejected the request — your Client ID may be invalid or the API is not enabled"
    info "Make sure the Gmail API is enabled in your Google Cloud project"
    ERRORS=$((ERRORS + 1))
else
    warn "Unexpected response ($RESPONSE) — credentials may still work, try connecting in Curier"
fi

# ── Step 5: Check Gmail API accessibility ──
echo ""
echo -e "${BOLD}5. Checking Gmail API availability${NC}"

GMAIL_RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" \
    "https://gmail.googleapis.com/gmail/v1/users/me/profile" \
    -H "Authorization: Bearer fake_token" \
    2>/dev/null || echo "000")

if [[ "$GMAIL_RESPONSE" == "000" ]]; then
    fail "Could not reach Gmail API — check your internet connection"
    ERRORS=$((ERRORS + 1))
elif [[ "$GMAIL_RESPONSE" == "401" ]]; then
    pass "Gmail API endpoint is reachable"
elif [[ "$GMAIL_RESPONSE" == "403" ]]; then
    warn "Gmail API returned 403 — make sure the Gmail API is enabled in your project"
    info "Go to: console.cloud.google.com → APIs & Services → Library → Gmail API → Enable"
else
    pass "Gmail API endpoint responded ($GMAIL_RESPONSE)"
fi

# ── Step 6: Check port 8080 ──
echo ""
echo -e "${BOLD}6. Checking port 8080 (OAuth callback)${NC}"

if lsof -i :8080 &> /dev/null; then
    warn "Port 8080 is currently in use — this may interfere with the OAuth callback"
    info "Run this to free it: lsof -ti:8080 | xargs kill -9"
    ERRORS=$((ERRORS + 1))
else
    pass "Port 8080 is available"
fi

# ── Summary ──
echo ""
echo "─────────────────────────────────────"

if [[ $ERRORS -eq 0 ]]; then
    echo -e "${GREEN}${BOLD}All checks passed!${NC} You're ready to connect Curier."
    echo ""
    echo "  Next steps:"
    echo "  1. Open Curier"
    echo "  2. Enter your Client ID, Client Secret, and Gmail address"
    echo "  3. Click 'Connect Gmail' and authorize in your browser"
else
    echo -e "${RED}${BOLD}$ERRORS issue(s) found.${NC} Review the errors above before connecting."
    echo ""
    echo "  Need help? See docs/GMAIL_SETUP.md for the full setup guide."
fi

echo ""
