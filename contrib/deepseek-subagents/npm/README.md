# codex-dp

`codex-dp` is the npm distribution of the
[Codex DeepSeek community fork](https://github.com/lyj96/codex-deepseek).
It keeps the main agent on OpenAI models and adds DeepSeek models such as
`deepseek-flash` and `deepseek-v4-pro` for external sub-agents.

## Install

```bash
npm install -g codex-dp
codex-dp setup
codex-dp
```

`codex-dp setup` downloads the installer from the matching GitHub Release,
checks its SHA-256 digest, configures the DeepSeek provider and API key, enables
`multi_agent_v2`, and can install the matching fork on Codex Desktop SSH hosts.
The npm package itself is intentionally small and does not duplicate the native
archives already hosted in GitHub Releases.

```bash
codex-dp setup --ssh-host devbox
codex-dp setup --update-remotes --yes
```

Supported platforms: Windows x64, Apple Silicon macOS, and Linux x64.
See the repository's
[Chinese quick start](https://github.com/lyj96/codex-deepseek/tree/deepseek/contrib/deepseek-subagents)
for complete setup and security notes.

This is a community fork and is not an official OpenAI package.
