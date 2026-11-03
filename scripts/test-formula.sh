#!/bin/sh
# Checks scripts/gen-formula.sh against fake checksums. No network.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/terms-formula-test.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

failures=0
pass() { printf 'ok   %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; failures=$((failures + 1)); }

hash() { printf '%064d' "$1"; }
{
    printf '%s  terms-aarch64-apple-darwin.tar.gz\n' "$(hash 1)"
    printf '%s  terms-x86_64-apple-darwin.tar.gz\n' "$(hash 2)"
    printf '%s  terms-aarch64-unknown-linux-musl.tar.gz\n' "$(hash 3)"
    printf '%s  terms-x86_64-unknown-linux-musl.tar.gz\n' "$(hash 4)"
} >"$work/SHA256SUMS"

gen() { sh "$root/scripts/gen-formula.sh" "$@"; }

if gen v1.2.3 Owner/Repo "$work/SHA256SUMS" >"$work/terms.rb" 2>"$work/err"; then
    ok=1
    for want in \
        'class Terms < Formula' \
        'version "1.2.3"' \
        'license "MIT"' \
        'homepage "https://github.com/Owner/Repo"' \
        'bin.install "terms"' \
        'terms init' \
        "url \"https://github.com/Owner/Repo/releases/download/v1.2.3/terms-aarch64-apple-darwin.tar.gz\"" \
        "url \"https://github.com/Owner/Repo/releases/download/v1.2.3/terms-x86_64-apple-darwin.tar.gz\"" \
        "url \"https://github.com/Owner/Repo/releases/download/v1.2.3/terms-aarch64-unknown-linux-musl.tar.gz\"" \
        "url \"https://github.com/Owner/Repo/releases/download/v1.2.3/terms-x86_64-unknown-linux-musl.tar.gz\"" \
        "sha256 \"$(hash 1)\"" "sha256 \"$(hash 2)\"" "sha256 \"$(hash 3)\"" "sha256 \"$(hash 4)\""; do
        grep -qF "$want" "$work/terms.rb" || { fail "formula lacks: $want"; ok=0; }
    done
    [ "$ok" = 1 ] && pass "formula has version, urls, checksums and install steps"
else
    fail "generation failed"; cat "$work/err" >&2
fi

# Each checksum must land under the matching platform block.
if awk '/on_macos/{os="mac"} /on_linux/{os="linux"} /on_arm/{cpu="arm"} /on_intel/{cpu="intel"}
        /sha256/{print os, cpu, $2}' "$work/terms.rb" | tr -d '"' | tr '\n' ';' \
    | grep -q "mac arm $(hash 1);mac intel $(hash 2);linux arm $(hash 3);linux intel $(hash 4);"; then
    pass "checksums are in the right OS/CPU blocks"
else
    fail "checksums are in the wrong blocks"
fi

if command -v ruby >/dev/null 2>&1; then
    if ruby -c "$work/terms.rb" >/dev/null 2>&1; then pass "formula is valid Ruby"; else fail "formula is not valid Ruby"; fi
else
    printf 'skip ruby syntax check: ruby not installed\n'
fi

grep -v 'x86_64-unknown-linux-musl' "$work/SHA256SUMS" >"$work/partial"
if gen v1.2.3 Owner/Repo "$work/partial" >/dev/null 2>"$work/err"; then fail "missing target was accepted"
elif grep -q 'x86_64-unknown-linux-musl' "$work/err"; then pass "missing checksum for a target is an error"
else fail "missing target error unclear"; fi

if gen 1.2.3 Owner/Repo "$work/SHA256SUMS" >/dev/null 2>&1; then fail "tag without leading v accepted"; else pass "tag must start with v"; fi

if [ "$failures" -ne 0 ]; then printf '%s check(s) failed\n' "$failures" >&2; exit 1; fi
echo "gen-formula.sh: all checks passed"
