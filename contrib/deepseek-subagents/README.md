# Codex DeepSeek 快速开始

这个社区 Fork 保持主 Agent 使用 OpenAI 模型，并允许子 Agent 使用
`deepseek-flash`、`deepseek-v4-pro` 等以 `deepseek` 开头的模型。

## 1. 下载

从 [Releases](https://github.com/lyj96/codex-deepseek/releases/latest)
下载对应平台的完整压缩包：

| 系统 | 文件 |
| --- | --- |
| Windows x64 | `codex-deepseek-package-x86_64-pc-windows-msvc.zip` |
| Windows ARM64 | `codex-deepseek-package-aarch64-pc-windows-msvc.zip` |
| macOS Intel | `codex-deepseek-package-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `codex-deepseek-package-aarch64-apple-darwin.tar.gz` |
| Linux x64 | `codex-deepseek-package-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 | `codex-deepseek-package-aarch64-unknown-linux-musl.tar.gz` |

解压后，入口位于 `bin/codex`；Windows 为 `bin/codex.exe`。
发布包附带 `SHA256SUMS`，且没有商业代码签名。macOS 首次运行若被 Gatekeeper
拦截，请在确认校验和后到“系统设置 → 隐私与安全性”中允许本次运行。

## 2. 配置

下载 Release 中的 `deepseek-worker.toml` 和 `deepseek-models.json`，保存到：

```text
~/.codex/agents/deepseek-worker.toml
~/.codex/agents/deepseek-models.json
```

在 `~/.codex/config.toml` 末尾加入：

```toml
[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/"
env_key = "DEEPSEEK_API_KEY"
wire_api = "responses"
supports_websockets = false
```

设置 DeepSeek API Key：

```powershell
# Windows PowerShell
[Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", "你的 API Key", "User")
```

```bash
# macOS / Linux
export DEEPSEEK_API_KEY="你的 API Key"
```

CLI 用户可直接运行解压目录中的 `bin/codex`。

Codex Desktop 用户将 `CODEX_CLI_PATH` 指向上述入口，然后完全退出并重新打开桌面端：

```powershell
# Windows PowerShell
[Environment]::SetEnvironmentVariable(
  "CODEX_CLI_PATH",
  "C:\你的绝对路径\bin\codex.exe",
  "User"
)
```

```bash
# macOS
launchctl setenv CODEX_CLI_PATH "/你的绝对路径/bin/codex"
launchctl setenv DEEPSEEK_API_KEY "你的 API Key"
```

## 3. 使用

直接告诉主 Agent：

> 使用 model 为 deepseek-flash、思考程度 max 的子 Agent 检查这个项目并汇报结果。

支持的思考程度为 `low`、`high`、`max`。不需要指定 `agent_type`。

发送给 DeepSeek 子 Agent 的任务内容和必要上下文会通过 DeepSeek API 处理。
