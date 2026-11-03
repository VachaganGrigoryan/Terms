#!/bin/sh
# Regenerate the README GIFs with VHS inside Docker. Nothing is installed on the host.
#   docs/demo/record.sh            # every tape
#   docs/demo/record.sh ls json    # only these tapes
set -eu
cd "$(dirname "$0")/../.."

case $(docker info --format '{{.Architecture}}') in
    aarch64 | arm64) arch=aarch64 ;;
    x86_64 | amd64) arch=x86_64 ;;
    *) echo "record.sh: unsupported Docker architecture" >&2; exit 1 ;;
esac
target=$arch-unknown-linux-musl

echo "Building terms for $target"
docker run --rm -v "$PWD":/src -w /src -e CARGO_TARGET_DIR=/src/target/demo rust:1 \
    sh -c "rustup target add $target >/dev/null 2>&1 && cargo build --release --locked --target $target"
mkdir -p docs/demo/.build docs/img
cp "target/demo/$target/release/terms" docs/demo/.build/terms
docker build -q -t terms-demo docs/demo >/dev/null

tapes=$*
if [ "$#" -eq 0 ]; then
    for file in docs/demo/*.tape; do
        name=$(basename "$file" .tape)
        # common and style are includes, not recordings.
        case $name in common | style) ;; *) tapes="$tapes $name" ;; esac
    done
fi
for name in $tapes; do
    echo "Recording $name"
    docker run --rm -v "$PWD/docs":/docs -w /docs/demo terms-demo "$name.tape"
done
ls -l docs/img
