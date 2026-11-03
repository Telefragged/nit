# Releases

## Public contract

`nix build .#contract` builds the public contract. `openapi.json` is the
OpenAPI document that the routes' `#[utoipa::path]` annotations produce, and
the router registers its routes from the same annotations. `cli.json` is the
syntax of every `nit` command: its flags, positionals and subcommands.
`nix build -f release --argstr last <tag> '^*'` compares it with the
contract at the release tag `last`, and fails when the version in
`Cargo.toml` is too small a step for the changes. Before 1.0.0, a breaking
change needs the next minor version and any other change the next patch.
`result` holds the kind of the changes, and `result-changes` lists them. `nix run .#bump-oasdiff`
moves the pinned oasdiff to its latest release.

## Making a release

A release starts from one edit: raise `workspace.package.version` in `Cargo.toml`. Then run `nix develop -c cargo check` to update `Cargo.lock` and `nix develop -c crate2nix generate` to update `Cargo.nix`, and open a PR. Run `nix run .#release-check` to see the result of the CI check before you push.

The `semver` job runs `nix run .#release-check` on every PR. When the version differs from the newest release tag, the script checks it against the contract changes since that tag (see "Public contract"). Release tags are the bare version, for example `0.2.0`.

After the merge, `.github/workflows/release.yml` runs the same check. When the commit is a release, it builds `.#nit-release`, creates the tag and the GitHub release, and uploads `nit-x86_64-linux` with its checksum and a build provenance attestation. To confirm that a binary came from that workflow, run:

```sh
gh attestation verify nit-x86_64-linux --repo Telefragged/nit
```

The repository settings carry two rulesets:

- `main`: no direct push and no force push, the `nix` and `semver` jobs are required status checks, and a PR must be up to date with `main` before it merges. A repository owned by a personal account has no merge queue.
- Tags `*.*.*`: no update and no deletion. A ruleset cannot exempt the workflow token, so tag creation stays open for the release workflow.
