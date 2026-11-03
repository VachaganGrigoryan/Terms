#!/bin/sh
# Install a prebuilt Terms release (macOS and Linux).
#
#   curl -fsSL https://raw.githubusercontent.com/VachaganGrigoryan/Terms/main/install.sh | sh
#
# Environment:
#   TERMS_VERSION      release tag such as v0.1.0 (default: latest)
#   TERMS_INSTALL_DIR  where to put the binary (default: $HOME/.local/bin)
#   TERMS_REPO         GitHub owner/name (default: VachaganGrigoryan/Terms)
#   TERMS_TARGET       override the detected target triple
#   TERMS_BASE_URL     download base URL (testing and mirrors); https is not enforced for it
#
# The archive is checked against the release's SHA256SUMS before anything is installed. That
# detects a corrupted or tampered download of the archive, not a compromised release: the sums
# come from the same place as the archive. Only the binary is written, to TERMS_INSTALL_DIR.
# No shell startup file is ever modified.
set -eu

repo=${TERMS_REPO:-VachaganGrigoryan/Terms}
version=${TERMS_VERSION:-latest}
dir=${TERMS_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}

say() { printf '%s\n' "$*"; }
die() { printf 'terms install: %s\n' "$*" >&2; exit 1; }

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case $os in
        Darwin) suffix=apple-darwin ;;
        Linux) suffix=unknown-linux-musl ;;
        *) die "unsupported operating system: $os (macOS and Linux only)" ;;
    esac
    case $arch in
        arm64 | aarch64) arch=aarch64 ;;
        x86_64 | amd64) arch=x86_64 ;;
        *) die "unsupported architecture: $arch" ;;
    esac
    # A shell running under Rosetta reports x86_64 on Apple Silicon; the native build is better.
    if [ "$os" = Darwin ] && [ "$arch" = x86_64 ] \
        && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
        arch=aarch64
    fi
    printf '%s-%s' "$arch" "$suffix"
}

target=${TERMS_TARGET:-$(detect_target)}

if [ -n "${TERMS_BASE_URL:-}" ]; then
    base=$TERMS_BASE_URL
    proto=
elif [ "$version" = latest ]; then
    base=https://github.com/$repo/releases/latest/download
    proto=https
else
    base=https://github.com/$repo/releases/download/$version
    proto=https
fi

fetch() { # fetch URL DEST
    if command -v curl >/dev/null 2>&1; then
        if [ -n "$proto" ]; then
            curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
        else
            curl -fsSL -o "$2" "$1"
        fi
    elif command -v wget >/dev/null 2>&1 && [ -n "$proto" ]; then
        wget -q -O "$2" "$1"
    else
        die "curl (or wget for https downloads) is required"
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
    else die "sha256sum or shasum is required to verify the download"; fi
}

tmp=$(mktemp -d "${TMPDIR:-/tmp}/terms-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM

archive=terms-$target.tar.gz
say "Installing Terms ($version) for $target"
fetch "$base/$archive" "$tmp/$archive" || die "no release archive for $target at $base/$archive"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || die "cannot download SHA256SUMS from $base"

expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1; exit }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || die "SHA256SUMS has no entry for $archive; refusing to install"
actual=$(sha256 "$tmp/$archive")
[ "$expected" = "$actual" ] || die "checksum mismatch for $archive (expected $expected, got $actual); refusing to install"

tar -xzf "$tmp/$archive" -C "$tmp" terms || die "cannot extract the terms binary from $archive"
mkdir -p "$dir" || die "cannot create $dir"
# Copy then rename so a running terms is never half-overwritten.
if ! { cp "$tmp/terms" "$dir/.terms.new.$$" && chmod 755 "$dir/.terms.new.$$" \
    && mv -f "$dir/.terms.new.$$" "$dir/terms"; }; then
    rm -f "$dir/.terms.new.$$"
    die "cannot write $dir/terms"
fi

say "Installed $("$dir/terms" --version) to $dir/terms"
case ":$PATH:" in
    *":$dir:"*) ;;
    *) say "Note: $dir is not on your PATH. Add it, for example:"
       say "  export PATH=\"$dir:\$PATH\"" ;;
esac
shell=$(basename "${SHELL:-}")
case $shell in zsh | bash) ;; *) shell=zsh ;; esac
rc=.${shell}rc
say "Optional shell integration (short names tl, ts, tg, tj ...): add this line to ~/$rc"
say "  eval \"\$(terms init $shell)\""
say "Nothing outside $dir was modified."
