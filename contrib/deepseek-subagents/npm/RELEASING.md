# Publishing `codex-dp`

Every tagged DeepSeek release builds three platform tarballs plus the root npm
package. npm publication is intentionally gated by the repository variable
`NPM_PUBLISH_ENABLED=true`.

## One-time npm setup

1. Sign in to npm and claim the unscoped `codex-dp` package with the first
   release. The package name must still be available at that moment.
2. In the npm package settings, add a GitHub Actions trusted publisher:
   - organization/user: `lyj96`
   - repository: `codex-deepseek`
   - workflow: `deepseek-release.yml`
3. In the GitHub repository settings, create the Actions variable
   `NPM_PUBLISH_ENABLED` with value `true`.

No long-lived `NPM_TOKEN` is used. The release workflow requests a short-lived
npm credential through GitHub OIDC and publishes platform versions serially,
then advances `latest` with the root package.

The platform versions use these dist-tags:

- `linux-x64`
- `darwin-arm64`
- `win32-x64`

The public version is `X.Y.Z-deepseek.N`; the native Codex build metadata stays
`X.Y.Z+deepseek.N`.
