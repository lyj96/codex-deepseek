# Mixed OpenAI + DeepSeek subagents for Codex Desktop

This fork keeps the desktop task on its normal OpenAI model while allowing an
explicit personal subagent role to use DeepSeek's official Responses API.

## What the patch changes

- A role file under `~/.codex/agents/` may select `model_provider` and a
  role-local `model_catalog_json`.
- Repository-defined agent roles cannot change provider or catalog. This keeps
  an untrusted repository from silently sending code to a third party.
- Cross-provider messages use ordinary plaintext Responses API user messages.
  OpenAI-only `agent_message` and `encrypted_content` items are not sent to
  DeepSeek.
- The official multi-agent v2 tools keep their OpenAI-validated encrypted
  schemas. This fork adds `spawn_external_agent`, `send_external_message`, and
  `followup_external_task` variants whose message arguments are locally
  readable so an external provider can receive them. Codex redacts those
  plaintext arguments from tool logs.
- A cross-provider child starts with a fresh context. The parent must put all
  necessary context in the delegated task.

## Install on Windows

Build `codex-cli` for `x86_64-pc-windows-gnu`, then run:

```bash
rustup target add x86_64-pc-windows-gnu
CARGO_PROFILE_RELEASE_LTO=false \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  CARGO_PROFILE_RELEASE_DEBUG=none \
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
  RUSTFLAGS='-C link-arg=-Wl,--no-keep-memory' \
  cargo build -p codex-cli --release --target x86_64-pc-windows-gnu
```

On Ubuntu/WSL, the linker is provided by the `gcc-mingw-w64-x86-64`
package. Then run in Windows PowerShell:

```powershell
& .\contrib\deepseek-subagents\install.ps1
& .\contrib\deepseek-subagents\set-deepseek-key.ps1
```

The installer downloads only the model catalog embedded in DeepSeek's official
Codex setup script. It does not replace the top-level Codex model, provider, or
catalog. It copies the custom CLI to `%LOCALAPPDATA%\CodexDeepSeek\current`,
sets the user-level `CODEX_CLI_PATH`, and backs up the existing Codex config.

Fully quit and reopen Codex Desktop. Keep the main task on an OpenAI model, then
ask it to use the `deepseek_worker` subagent for a self-contained task.

Example:

> Use the deepseek_worker subagent to inspect this repository for duplicated
> retry logic. Return findings only; do not edit files.

To roll back the most recent installation:

```powershell
& .\contrib\deepseek-subagents\restore.ps1
```

The key helper never prints the key, but Windows user environment variables are
stored as plaintext in the user's profile. Do not paste API keys into chat or
commit them to the repository.
