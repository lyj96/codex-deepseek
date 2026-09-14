# Codex DeepSeek 快速开始

这个社区 Fork 保持主 Agent 使用 OpenAI 模型，并允许主 Agent 通过
`spawn_external_agent` 拉起 `deepseek-flash`、`deepseek-v4-pro` 等 DeepSeek 子 Agent。
不需要配置或指定 `deepseek_worker`。

## 一键安装

安装器会下载最新版、校验 SHA-256、配置 DeepSeek provider，并提示输入 API Key。

Windows x64（PowerShell）：

```powershell
irm https://github.com/lyj96/codex-deepseek/releases/latest/download/install-windows.ps1 | iex
```

Apple Silicon macOS：

```bash
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-macos.sh | bash
```

Linux x64：

```bash
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-linux.sh | bash
```

需要指定安装目录和 Key 时：

```powershell
# Windows
$s="$env:TEMP\install-codex-deepseek.ps1"; irm https://github.com/lyj96/codex-deepseek/releases/latest/download/install-windows.ps1 -OutFile $s; & $s -InstallDir "D:\CodexDeepSeek" -DeepSeekKey "你的 Key"
```

```bash
# macOS
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-macos.sh | bash -s -- --install-dir "$HOME/Applications/CodexDeepSeek" --deepseek-key "你的 Key"

# Linux
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-linux.sh | bash -s -- --install-dir "$HOME/apps/codex-deepseek" --deepseek-key "你的 Key"
```

直接在命令行传 Key 会进入 Shell 历史；个人电脑建议省略 `DeepSeekKey` / `--deepseek-key`，使用安装器的隐藏输入。

安装完成后完全退出并重新打开 Codex Desktop。CLI 命令为：

```text
codex-deepseek
```

## 使用

直接告诉主 Agent：

> 使用 model 为 deepseek-flash、思考程度 max 的子 Agent 检查这个项目并汇报结果。

支持 `low`、`high`、`max`。主 Agent 会使用 `spawn_external_agent`；后续可通过
`send_external_message` 和 `followup_external_task` 与同一个 DeepSeek 子 Agent 继续交互。

发送给 DeepSeek 子 Agent 的任务内容和必要上下文会通过 DeepSeek API 处理。

## 支持的平台

| 系统 | Release 包 |
| --- | --- |
| Windows x64 | `codex-deepseek-package-x86_64-pc-windows-msvc.zip` |
| Apple Silicon macOS | `codex-deepseek-package-aarch64-apple-darwin.tar.gz` |
| Linux x64 | `codex-deepseek-package-x86_64-unknown-linux-musl.tar.gz` |

发布包没有商业代码签名。macOS 首次运行若被 Gatekeeper 拦截，请先核对 Release 中的
`SHA256SUMS`，再到“系统设置 → 隐私与安全性”允许运行。

## 官方 Codex 更新后怎么办

安装器把 Fork 放在独立目录，并让 Codex Desktop 通过稳定的 `CODEX_CLI_PATH` 使用它；
官方 App 或 CLI 更新不会覆盖这份程序。用户配置和模型目录也保存在 `~/.codex` 下。

但 Codex App 与 CLI 的内部协议可能随版本变化。我们会按上游 Codex 版本发布对应版本；
更新 Codex App 后如果出现不兼容，重新运行上面的一键安装命令即可升级到最新 Fork。
