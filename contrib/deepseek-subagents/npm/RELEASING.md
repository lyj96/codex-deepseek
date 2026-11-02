# Publishing `codex-dp`

Every tagged DeepSeek release publishes one small npm installer package. Native
Windows, macOS, and Linux archives stay in GitHub Releases; `codex-dp setup`
downloads the matching archive and verifies its SHA-256 digest. npm publication
is intentionally gated by the repository variable `NPM_PUBLISH_ENABLED=true`.

## One-time npm setup

1. Sign in to npm and claim the unscoped `codex-dp` package with the first
   release. Publish the small root tarball first with an interactive OTP; the
   package name must still be available at that moment.
2. In the npm package settings, add a GitHub Actions trusted publisher:
   - organization/user: `lyj96`
   - repository: `codex-deepseek`
   - workflow: `deepseek-release.yml`
3. In the GitHub repository settings, create the Actions variable
   `NPM_PUBLISH_ENABLED` with value `true`.

For an existing release, run `deepseek-release` manually with `release_tag` set
to the existing tag. This npm-only mode builds and publishes only the thin npm
installer without rebuilding Rust.

If a published npm wrapper needs a packaging-only correction, set
`npm_package_version` to `X.Y.Z-deepseek.N-npm.M`. That version still downloads
the native `codex-vX.Y.Z-deepseek.N` GitHub Release.

No long-lived `NPM_TOKEN` is used. The release workflow requests a short-lived
npm credential through GitHub OIDC and advances `latest` with the installer.
Normal npm versions are `X.Y.Z-deepseek.N`; the native Codex build metadata is
`X.Y.Z+deepseek.N`.
