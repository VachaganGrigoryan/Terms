#!/bin/sh
# Builds the throwaway world the demos run in. Runs once at image build time; nothing here is real data.
set -eu
export HOME=/root
git config --global user.name "Terms Demo"
git config --global user.email "demo@example.invalid"
git config --global init.defaultBranch main

mkdir -p /demo/project/src /demo/project/docs /demo/website /demo/api /demo/archive
cd /demo/project
cat > Cargo.toml <<'T'
[package]
name = "greeter"
version = "0.2.0"
T
cat > README.md <<'T'
# greeter

A tiny example project used by the Terms demos.
T
cat > .env.example <<'T'
GREETING=hello
T
cat > src/main.rs <<'T'
// TODO: parse command-line arguments
fn main() {
    println!("{}", greeter::greeting("world"));
}
T
cat > src/lib.rs <<'T'
// TODO: support more languages
pub fn greeting(name: &str) -> String {
    format!("hello, {name}")
}
// FIXME: handle empty names
T
cat > src/util.rs <<'T'
pub fn shout(text: &str) -> String {
    text.to_uppercase()
}
T
cat > docs/guide.md <<'T'
# Guide

TODO: write the guide.
T

cat > /demo/data.json <<'T'
[
  {"id": 1, "fish": "tuna", "qty": 3},
  {"id": 2, "fish": "salmon", "qty": 1},
  {"id": 3, "fish": "tuna", "qty": 7},
  {"id": 4, "fish": "trout", "qty": 2}
]
T

mkdir -p /demo/repo/src
cd /demo/repo
git init -q
printf '# repo\n\nDemo repository.\n' > README.md
printf 'pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n' > src/lib.rs
git add -A
GIT_AUTHOR_DATE='2026-01-10T10:00:00Z' GIT_COMMITTER_DATE='2026-01-10T10:00:00Z' git commit -qm "Initial commit"
printf 'pub fn sub(a: i32, b: i32) -> i32 {\n    a - b\n}\n' >> src/lib.rs
git add -A
GIT_AUTHOR_DATE='2026-01-12T09:30:00Z' GIT_COMMITTER_DATE='2026-01-12T09:30:00Z' git commit -qm "Add sub"
printf 'Run the tests before committing.\n' >> README.md
printf 'ideas\n- multiply\n- divide\n' > notes.txt

# Directories the jump demo knows about, visited a different number of times.
for _ in 1 2 3 4; do terms jump add /demo/project; done
for _ in 1 2 3; do terms jump add /demo/repo; done
for _ in 1 2; do terms jump add /demo/website; done
terms jump add /demo/api
terms jump add /demo/archive
