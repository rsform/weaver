#!/usr/bin/env bash
# Test scripts for weaver-index XRPC endpoints

set -euo pipefail

BASE_URL="${INDEXER_URL:-http://localhost:3000}"
DID="did:plc:yfvwmnlztr4dwkb7hwz55r2g"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() { echo -e "${BLUE}==>${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1"; }

# Health check
test_health() {
    info "Testing health endpoint..."
    curl -s "${BASE_URL}/xrpc/_health" | jq .
}

# Get profile
test_get_profile() {
    info "Testing sh.weaver.actor.getProfile..."
    curl -s "${BASE_URL}/xrpc/sh.weaver.actor.getProfile?actor=${DID}" | jq .
}

# Resolve notebook
test_resolve_notebook() {
    local name="${1:-weaver}"
    info "Testing sh.weaver.notebook.resolveNotebook (name=${name})..."
    curl -s "${BASE_URL}/xrpc/sh.weaver.notebook.resolveNotebook?actor=${DID}&name=${name}" | jq .
}

# Get entry by URI
test_get_entry() {
    local rkey="${1:-3m7tg3ni77tqx}"
    local uri="at://${DID}/sh.weaver.notebook.entry/${rkey}"
    info "Testing sh.weaver.notebook.getEntry (rkey=${rkey})..."
    curl -s "${BASE_URL}/xrpc/sh.weaver.notebook.getEntry?uri=$(urlencode "${uri}")" | jq .
}

# Resolve entry by name
test_resolve_entry() {
    local notebook="${1:-weaver}"
    local entry="${2:-drafts_privacy}"
    info "Testing sh.weaver.notebook.resolveEntry (notebook=${notebook}, entry=${entry})..."
    curl -s "${BASE_URL}/xrpc/sh.weaver.notebook.resolveEntry?actor=${DID}&notebook=${notebook}&entry=${entry}" | jq .
}

# URL encode helper
urlencode() {
    python3 -c "import urllib.parse; print(urllib.parse.quote('$1', safe=''))"
}

# Test all entry rkeys
test_all_entries() {
    local rkeys=(
        "3m7tg3ni77tqx"
        "3m7gtl3v4t3kn"
        "3m7ekja42a32v"
        "3m746pdxlldfq"
        "3m6wvayeoqdx4"
        "3m6ug3zrwb22v"
        "3m6sy3qur622v"
        "3m6mnvrkoeq2v"
        "3m5mepkowvy2a"
        "3m4rbphjzt62b"
        "3m4oy5go4742b"
        "3m4okwb7wp42b"
        "3m4ojkfioom2b"
    )

    for rkey in "${rkeys[@]}"; do
        test_get_entry "$rkey"
        echo
    done
}

# Run all tests
test_all() {
    test_health
    echo
    test_get_profile
    echo
    test_resolve_notebook "weaver"
    echo
    test_resolve_entry "weaver" "drafts_privacy"
    echo
    test_get_entry "3m7tg3ni77tqx"
}

# Main
case "${1:-all}" in
    health)
        test_health
        ;;
    profile)
        test_get_profile
        ;;
    notebook)
        test_resolve_notebook "${2:-weaver}"
        ;;
    entry)
        test_get_entry "${2:-3m7tg3ni77tqx}"
        ;;
    resolve)
        test_resolve_entry "${2:-weaver}" "${3:-drafts_privacy}"
        ;;
    entries)
        test_all_entries
        ;;
    all)
        test_all
        ;;
    *)
        echo "Usage: $0 {health|profile|notebook [name]|entry [rkey]|resolve [notebook] [entry]|entries|all}"
        echo
        echo "Environment:"
        echo "  INDEXER_URL  Base URL (default: http://localhost:3000)"
        exit 1
        ;;
esac
