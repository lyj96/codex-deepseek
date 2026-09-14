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
packages build successfully.
