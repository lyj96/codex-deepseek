# Maintaining the DeepSeek fork

`main` mirrors the upstream Fork branch. `deepseek` is the release branch and
is based on the latest stable `rust-vX.Y.Z` tag from `openai/codex`.

Release tags use:

```text
codex-vX.Y.Z-deepseek.N
```

`N` starts at `1` for every new upstream Codex release and increments for fixes
that keep the same upstream version. Packages record the SemVer build metadata
`X.Y.Z+deepseek.N`.

To update:

```bash
git fetch upstream --tags
git switch deepseek
git merge rust-vX.Y.Z
```

Resolve conflicts, run the DeepSeek tests, then create and push the release tag:

```bash
git tag -a codex-vX.Y.Z-deepseek.1 -m "Codex X.Y.Z + DeepSeek r1"
git push origin deepseek codex-vX.Y.Z-deepseek.1
```

The `deepseek-release` workflow publishes only after all supported platform
packages build successfully. The supported release targets are intentionally
limited to:

- `x86_64-pc-windows-msvc`
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-musl`

The installers keep the fork outside the official Codex installation and set a
stable `CODEX_CLI_PATH`. Upstream updates therefore do not overwrite the fork,
but app-server protocol changes can still require a matching fork build. For
each new stable upstream tag, merge it, reset the DeepSeek revision to `.1`, and
publish the matching release promptly.
