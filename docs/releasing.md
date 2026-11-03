# Releasing Terms

A release is a `vX.Y.Z` tag on `main`. The tag triggers `.github/workflows/release.yml`; nothing is built or uploaded by hand.

## Cut a release

1. Make sure `main` is green (the CI workflow) and the working tree is clean.
2. Bump `version` in `Cargo.toml`, then run `cargo update -p terms --offline`. That changes only the `terms` entry in `Cargo.lock` (a stale lock makes every `--locked` build fail). Commit both files.
3. Merge to `main`, then tag that exact commit and push the tag:

   ```sh
   git tag -a v0.1.0 -m "Terms v0.1.0"
   git push origin v0.1.0
   ```

4. Watch it: `gh run watch` (or the Actions tab). On success the release page has four archives, `SHA256SUMS` and `terms.rb`.
5. Publish the Homebrew formula (needs your own `gh` login with push access to the tap):

   ```sh
   scripts/update-tap.sh v0.1.0 --dry-run   # print the formula first
   scripts/update-tap.sh v0.1.0             # commit Formula/terms.rb to the tap
   ```

6. Smoke-test as a user would, in a throwaway home:

   ```sh
   TERMS_INSTALL_DIR=$(mktemp -d) sh -c 'curl -fsSL https://raw.githubusercontent.com/VachaganGrigoryan/Terms/main/install.sh | sh'
   ```

## What the workflow does

- **verify**: the tag must equal the `Cargo.toml` version and be reachable from `main`.
- **ci**: reruns the full CI workflow on the tagged commit.
- **build**: four targets, each built with `--locked`, tested (`cargo test --release`) where the runner can execute it, smoke-run, and packaged as `terms-<target>.tar.gz` containing `terms`, `LICENSE` and `README.md`:

  | Target | Runner |
  |---|---|
  | `aarch64-apple-darwin` | `macos-latest` |
  | `x86_64-apple-darwin` | `macos-latest` (cross-built; not executed in CI if Rosetta is absent) |
  | `x86_64-unknown-linux-musl` | `ubuntu-latest` (static) |
  | `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` (static) |

- **checksums and formula**: `SHA256SUMS` over the four archives, and `terms.rb` from `scripts/gen-formula.sh`.
- **publish** (tags only): `gh release create` with generated notes. The release is deliberately **not** marked prerelease, because GitHub's `releases/latest/download/...` URL, which `install.sh` uses by default, ignores prereleases.

Asset names carry no version (`terms-<target>.tar.gz`) so that `releases/latest/download/` works. The version lives in the tag.

## Dry run

Running the **Release** workflow from the Actions tab on a branch builds, tests and packages everything and uploads `release-files` as a workflow artifact for seven days, but publishes nothing. Use it after changing the workflow, the build targets or the scripts.

## Recording the README GIFs

```sh
docs/demo/record.sh          # every tape
docs/demo/record.sh git ls   # only these
```

The script builds a static Linux `terms` in Docker, bakes it with throwaway fixtures into an image pinned to a VHS digest, and writes `docs/img/*.gif`. Nothing is installed on your machine and no real files appear in the recordings. Re-record after any change to output that a tape shows, and keep each GIF under about 700 KB.

## Local checks for the release scripts

```sh
sh scripts/test-install.sh     # install.sh against a fake local release
sh scripts/test-formula.sh     # formula generation
shellcheck install.sh scripts/*.sh docs/demo/*.sh
```

## Known limits

- The binaries are not code-signed or notarized. Installing with `curl` or Homebrew avoids macOS quarantine; archives downloaded in a browser need `xattr -d com.apple.quarantine terms`.
- `install.sh` verifies a checksum served next to the archive, so it detects corruption or tampering with the archive, not a compromised release.
- Updating the tap is a manual step (`scripts/update-tap.sh`). Automating it needs a personal access token stored as a repository secret.
