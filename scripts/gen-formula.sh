#!/bin/sh
# Print the Homebrew formula for a release.
#   scripts/gen-formula.sh vX.Y.Z OWNER/REPO path/to/SHA256SUMS > terms.rb
set -eu

[ "$#" -eq 3 ] || { echo "usage: gen-formula.sh vX.Y.Z OWNER/REPO SHA256SUMS" >&2; exit 2; }
tag=$1
repo=$2
sums=$3
case $tag in v[0-9]*) ;; *) echo "gen-formula: tag must look like v1.2.3, got '$tag'" >&2; exit 2 ;; esac
version=${tag#v}

checksum() { # checksum TARGET -> 64 hex digits from the sums file, or an error naming TARGET
    value=$(awk -v f="terms-$1.tar.gz" '$2 == f || $2 == "*" f { print $1; exit }' "$sums")
    case $value in
        *[!0-9a-f]* | '') echo "gen-formula: no valid checksum for $1 in $sums" >&2; exit 1 ;;
    esac
    [ "${#value}" -eq 64 ] || { echo "gen-formula: no valid checksum for $1 in $sums" >&2; exit 1; }
    printf '%s' "$value"
}

mac_arm=$(checksum aarch64-apple-darwin)
mac_intel=$(checksum x86_64-apple-darwin)
linux_arm=$(checksum aarch64-unknown-linux-musl)
linux_intel=$(checksum x86_64-unknown-linux-musl)
base=https://github.com/$repo/releases/download/$tag

cat <<EOF
class Terms < Formula
  desc "Independently implemented terminal toolkit: ls, search, json, pick, jump, history, git"
  homepage "https://github.com/$repo"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "$base/terms-aarch64-apple-darwin.tar.gz"
      sha256 "$mac_arm"
    end
    on_intel do
      url "$base/terms-x86_64-apple-darwin.tar.gz"
      sha256 "$mac_intel"
    end
  end

  on_linux do
    on_arm do
      url "$base/terms-aarch64-unknown-linux-musl.tar.gz"
      sha256 "$linux_arm"
    end
    on_intel do
      url "$base/terms-x86_64-unknown-linux-musl.tar.gz"
      sha256 "$linux_intel"
    end
  end

  def install
    bin.install "terms"
  end

  def caveats
    <<~EOS
      Optional shell integration (short names tl, ts, tg, tj ...): add this line to ~/.zshrc or ~/.bashrc
        eval "\$(terms init zsh)"   # or: terms init bash
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/terms --version")
  end
end
EOF
