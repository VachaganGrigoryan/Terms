#!/bin/sh
# Exercises install.sh against a fake local release. No network, no writes outside a temp dir.
# Usage: scripts/test-install.sh   (uses $TERMS_BIN, else target/release/terms, building it if absent)
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bin=${TERMS_BIN:-$root/target/release/terms}
if [ ! -x "$bin" ]; then
    (cd "$root" && cargo build --release --locked)
fi
[ -x "$bin" ] || { echo "no terms binary at $bin" >&2; exit 1; }

work=$(mktemp -d "${TMPDIR:-/tmp}/terms-install-test.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

failures=0
pass() { printf 'ok   %s\n' "$1"; }
fail() { printf 'FAIL %s\n' "$1" >&2; failures=$((failures + 1)); }

target=fake-target
release=$work/release
mkdir -p "$release" "$work/stage"
cp "$bin" "$work/stage/terms"
cp "$root/LICENSE" "$root/README.md" "$work/stage/"
(cd "$work/stage" && tar -czf "$release/terms-$target.tar.gz" terms LICENSE README.md)
sum() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
printf '%s  terms-%s.tar.gz\n' "$(sum "$release/terms-$target.tar.gz")" "$target" >"$release/SHA256SUMS"

# install DIR [VAR=VALUE...]: run install.sh with a clean HOME; prints nothing, returns its status.
install() {
    dir=$1
    shift
    mkdir -p "$work/home"
    env -i PATH="$PATH" HOME="$work/home" TERMS_BASE_URL="file://$release" \
        TERMS_INSTALL_DIR="$dir" TERMS_TARGET="$target" "$@" \
        sh "$root/install.sh" >"$work/out" 2>"$work/err"
}

# 1. A good install works, leaves no files outside the install dir, and the binary runs.
if install "$work/bin1" && [ -x "$work/bin1/terms" ] \
    && "$work/bin1/terms" --version | grep -q '^terms ' \
    && [ -z "$(ls -A "$work/home")" ]; then
    pass "installs a verified release and touches nothing else"
else
    fail "good install"; cat "$work/err" >&2
fi

# 2. The hint tells the user how to enable the shell integration.
if grep -q 'terms init' "$work/out"; then pass "prints the terms init hint"; else fail "init hint missing"; fi

# 3. Installing again replaces the binary.
if install "$work/bin1" && "$work/bin1/terms" --version >/dev/null; then pass "reinstall over an existing binary"
else fail "reinstall"; fi

# 4. A tampered archive is refused and nothing is installed.
cp "$release/terms-$target.tar.gz" "$work/good.tar.gz"
printf 'tampered' >>"$release/terms-$target.tar.gz"
if install "$work/bin2"; then fail "tampered archive was accepted"
elif [ -e "$work/bin2/terms" ]; then fail "tampered archive left a binary behind"
elif grep -qi 'checksum' "$work/err"; then pass "checksum mismatch refused"
else fail "mismatch error did not mention checksum"; cat "$work/err" >&2; fi
cp "$work/good.tar.gz" "$release/terms-$target.tar.gz"

# 5. A release without an entry for the archive is refused.
cp "$release/SHA256SUMS" "$work/SHA256SUMS.good"
: >"$release/SHA256SUMS"
if install "$work/bin3"; then fail "missing checksum entry was accepted"
elif [ -e "$work/bin3/terms" ]; then fail "missing entry left a binary behind"
else pass "missing checksum entry refused"; fi
cp "$work/SHA256SUMS.good" "$release/SHA256SUMS"

# 6. A target with no published archive fails clearly.
if install "$work/bin4" TERMS_TARGET=no-such-target; then fail "unknown target was accepted"
elif [ -e "$work/bin4/terms" ]; then fail "unknown target left a binary behind"
else pass "unknown target refused"; fi

# 7. Unsupported operating systems are rejected before any download (uname shimmed).
mkdir -p "$work/shim"
printf '#!/bin/sh\necho FreeBSD\n' >"$work/shim/uname"
chmod +x "$work/shim/uname"
mkdir -p "$work/home"
if env -i PATH="$work/shim:$PATH" HOME="$work/home" TERMS_BASE_URL="file://$release" \
    TERMS_INSTALL_DIR="$work/bin5" sh "$root/install.sh" >"$work/out" 2>"$work/err"; then
    fail "FreeBSD was accepted"
elif grep -qi 'unsupported' "$work/err" && [ ! -e "$work/bin5/terms" ]; then pass "unsupported OS rejected"
else fail "unsupported OS message"; cat "$work/err" >&2; fi

# 8. Host detection maps this machine to a published target name (no TERMS_TARGET override).
case "$(uname -s)-$(uname -m)" in
    Darwin-arm64|Darwin-x86_64) host=apple-darwin ;;
    Linux-x86_64|Linux-aarch64|Linux-arm64) host=unknown-linux-musl ;;
    *) host= ;;
esac
if [ -n "$host" ]; then
    case "$(uname -m)" in x86_64|amd64) arch=x86_64 ;; *) arch=aarch64 ;; esac
    # Apple Silicon shells running under Rosetta report x86_64 but must still get arm64.
    if [ "$(uname -s)" = Darwin ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then arch=aarch64; fi
    detected=$arch-$host
    cp "$release/terms-$target.tar.gz" "$release/terms-$detected.tar.gz"
    printf '%s  terms-%s.tar.gz\n' "$(sum "$release/terms-$detected.tar.gz")" "$detected" >"$release/SHA256SUMS"
    mkdir -p "$work/home"
    if env -i PATH="$PATH" HOME="$work/home" TERMS_BASE_URL="file://$release" \
        TERMS_INSTALL_DIR="$work/bin6" sh "$root/install.sh" >"$work/out" 2>"$work/err" \
        && [ -x "$work/bin6/terms" ]; then pass "detects this host as $detected"
    else fail "host detection ($detected)"; cat "$work/err" >&2; fi
else
    printf 'skip host detection: unsupported test host\n'
fi

if [ "$failures" -ne 0 ]; then
    printf '%s check(s) failed\n' "$failures" >&2
    exit 1
fi
echo "install.sh: all checks passed"
