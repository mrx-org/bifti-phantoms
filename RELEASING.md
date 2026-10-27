# Releasing the packages

Each implementation is versioned on its own, independently of the format. Each one
has its own tag prefix, so the tags never collide with each other or with the
format's `v*` tags:

| Package | Published to | Tag | Triggered by |
|---------|--------------|-----|--------------|
| Python `bifti` | [PyPI](https://pypi.org/project/bifti/) | `python-v<version>` | pushing the tag |
| Rust `bifti` | [crates.io](https://crates.io/crates/bifti) | `rust-v<version>` | pushing the tag |
| Julia `BiftiPhantoms` | [General registry](https://juliahub.com/ui/Packages/General/BiftiPhantoms) | `BiftiPhantoms-v<version>` | a `@JuliaRegistrator` comment; see [its README](julia/BiftiPhantoms/README.md#releasing) |
| MATLAB / Octave | not published | - | - |

## Python and Rust

1. Bump `version` in [`python/bifti/pyproject.toml`](python/bifti/pyproject.toml) or
   [`rust/bifti/Cargo.toml`](rust/bifti/Cargo.toml) (semver; while `0.x`, a minor
   bump marks a breaking change). For Python, run `uv lock` so `uv.lock` picks up the
   new version. Merge to `main` with the Test workflow green.
2. Tag that commit on `main` and push the tag:
   ```sh
   git tag python-v1.0.1   # or rust-v0.1.1
   git push origin python-v1.0.1
   ```
3. The [Python](.github/workflows/release-python.yml) or
   [Rust](.github/workflows/release-rust.yml) release workflow checks the tag against
   the version in the manifest, runs the tests, publishes the package and creates
   the matching GitHub release. If the tag and the version do not match, nothing is
   published: delete the tag (`git push --delete origin <tag>`), fix it and tag again.

A published version can never be uploaded again, on either registry. A broken
release is fixed by publishing a new version and yanking the broken one.

## One-time setup

Both registries use Trusted Publishing: the workflow proves its identity to the
registry with a short-lived GitHub OIDC token, so the repository stores no API
tokens. This needs, once:

**GitHub:** under *Settings → Environments*, create the environments `pypi` and
`crates-io`. Optionally restrict them to tags matching `python-v*` / `rust-v*`, or add
required reviewers so every release needs a manual approval.

**PyPI:** a project can be set up before its first upload. Log in to PyPI, open
*Your projects → Publishing → Add a new pending publisher* and enter:

| Field | Value |
|-------|-------|
| PyPI project name | `bifti` |
| Owner | `mrx-org` |
| Repository name | `bifti-phantoms` |
| Workflow name | `release-python.yml` |
| Environment name | `pypi` |

The first `python-v*` tag then creates the project.

**crates.io:** a trusted publisher can only be added to a crate that already
exists, so publish the first version by hand, from a clean checkout of the tagged
commit:

```sh
cd rust/bifti
cargo login          # paste an API token from https://crates.io/settings/tokens
cargo publish
```

Then, on the crate's page under *Settings → Trusted Publishing*, add a GitHub
publisher with owner `mrx-org`, repository `bifti-phantoms`, workflow
`release-rust.yml` and environment `crates-io`. Every later `rust-v*` tag then
publishes through CI. (If you also push the first version's tag, its `publish` job
fails because that version already exists; that failure is expected and harmless.) The crate settings can also
enforce Trusted Publishing only, rejecting uploads with API tokens.

Add the other maintainers as owners on both registries
(`cargo owner --add github:mrx-org:<team>` for a GitHub team on crates.io), so
the packages do not depend on one account.
