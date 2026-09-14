# Publishing `codex-dp`

Every tagged DeepSeek release builds three platform tarballs plus the root npm
package. npm publication is intentionally gated by the repository variable
`NPM_PUBLISH_ENABLED=true`.

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

For the first release, run `deepseek-release` manually with `release_tag` set to
the existing tag. This npm-only mode downloads the four tarballs already stored
in the GitHub Release and publishes them without rebuilding Rust. It safely
skips the root version claimed in step 1.

No long-lived `NPM_TOKEN` is used. The release workflow requests a short-lived
npm credential through GitHub OIDC and publishes platform versions serially,
then advances `latest` with the root package.

The platform versions use these dist-tags:

- `linux-x64`
- `darwin-arm64`
- `win32-x64`

The public version is `X.Y.Z-deepseek.N`; the native Codex build metadata stays
`X.Y.Z+deepseek.N`.
