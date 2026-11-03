#!/bin/sh
# Publish a release's Homebrew formula to the tap, using your own `gh` login (no stored token).
#   scripts/update-tap.sh vX.Y.Z            # commit Formula/terms.rb to the tap's default branch
#   scripts/update-tap.sh vX.Y.Z --dry-run  # print the formula, change nothing
# Environment: TERMS_REPO (default VachaganGrigoryan/Terms), TERMS_TAP (default VachaganGrigoryan/homebrew-tap)
set -eu

tag=${1:-}
mode=${2:-}
case $tag in v[0-9]*) ;; *) echo "usage: update-tap.sh vX.Y.Z [--dry-run]" >&2; exit 2 ;; esac
case $mode in '' | --dry-run) ;; *) echo "usage: update-tap.sh vX.Y.Z [--dry-run]" >&2; exit 2 ;; esac
repo=${TERMS_REPO:-VachaganGrigoryan/Terms}
tap=${TERMS_TAP:-VachaganGrigoryan/homebrew-tap}
path=Formula/terms.rb

tmp=$(mktemp -d "${TMPDIR:-/tmp}/terms-tap.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM

gh release download "$tag" -R "$repo" -p terms.rb -D "$tmp" \
    || { echo "update-tap: release $tag has no terms.rb asset (is the release workflow finished?)" >&2; exit 1; }
grep -q "version \"${tag#v}\"" "$tmp/terms.rb" \
    || { echo "update-tap: terms.rb does not match $tag; refusing to publish" >&2; exit 1; }

if [ "$mode" = --dry-run ]; then
    cat "$tmp/terms.rb"
    echo "update-tap: dry run, nothing pushed to $tap" >&2
    exit 0
fi

content=$(base64 <"$tmp/terms.rb" | tr -d '\n')
if sha=$(gh api "repos/$tap/contents/$path" --jq .sha 2>/dev/null); then
    gh api -X PUT "repos/$tap/contents/$path" -f message="terms $tag" -f content="$content" -f sha="$sha" --jq .commit.html_url
else
    gh api -X PUT "repos/$tap/contents/$path" -f message="terms $tag" -f content="$content" --jq .commit.html_url
fi
echo "Published terms $tag to $tap. Install with: brew install ${tap%%/*}/${tap#*/homebrew-}/terms"
