# Codex DeepSeek 快速开始

这个社区 Fork 保持主 Agent 使用 OpenAI 模型，并允许主 Agent 通过
`spawn_external_agent` 拉起 `deepseek-flash`、`deepseek-v4-pro` 等 DeepSeek 子 Agent。
不需要配置或指定 `deepseek_worker`。

## 选择安装方式

- **只使用 CLI：推荐 npm。** 安装、更新最简单，执行 `npm install -g codex-dp@latest` 即可。
- **使用 Codex Desktop 或 SSH 远程项目：推荐一键安装脚本。** 脚本会设置桌面端需要的
  `CODEX_CLI_PATH`，并可发现、登记和更新远程服务器。

两种方式安装后的 CLI 命令都统一为 `codex-dp`。

## 纯 CLI：npm 安装（推荐）

已安装 Node.js 18+ 时，三平台都可以使用：

```bash
npm install -g codex-dp
codex-dp setup
codex-dp
```

`codex-dp setup` 会配置 DeepSeek、启用 `multi_agent_v2`，并提示输入 API Key。
npm 包本身是小型安装器；三平台二进制仍从对应 GitHub Release 下载并校验
SHA-256，不会在 npm 中重复存储大包。
## Desktop / SSH：一键安装脚本（推荐）

安装器会下载最新版、校验 SHA-256、配置 DeepSeek provider、启用 `multi_agent_v2`，
并提示输入 API Key。

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

安装完成后完全退出并重新打开 Codex Desktop。三个系统的 CLI 命令统一为：

```text
codex-dp
```

旧版命令 `codex-deepseek` 暂时保留为兼容别名，新文档和脚本输出统一使用 `codex-dp`。

## SSH 远程项目

Codex Desktop 的 SSH 项目使用远程主机上的 `codex`。目前远程自动安装支持
Linux x86_64 服务器。首次安装时，安装器会优先把本机已有的 DeepSeek Key 通过加密的
SSH 标准输入传给远程安装器；Key 不会出现在命令参数或日志中，并会独立保存在远程。

先查看本机 `~/.ssh/config` 中可发现的主机：

```powershell
# Windows
$s="$env:TEMP\install-codex-deepseek.ps1"; irm https://github.com/lyj96/codex-deepseek/releases/latest/download/install-windows.ps1 -OutFile $s; & $s -DiscoverSsh
```

```bash
# macOS
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-macos.sh | bash -s -- --discover-ssh

# Linux
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-linux.sh | bash -s -- --discover-ssh
```

首次在指定服务器安装并登记：

```powershell
# Windows
& $s -SshHost devbox
```

```bash
# macOS 使用 install-macos.sh
bash install-macos.sh --ssh-host devbox

# Linux 使用 install-linux.sh
bash install-linux.sh --ssh-host devbox
```

已登记主机保存在 Windows 的 `%APPDATA%\CodexDeepSeek\ssh-hosts`，或 macOS/Linux 的
`~/.config/codex-deepseek/ssh-hosts`。

如果本机还没有 Key，安装器会通过 SSH 在远程终端隐藏提示输入。随后它会备份远程原有的
`~/.local/bin/codex`，让该入口指向 Codex DeepSeek，并显式启用 `multi_agent_v2`。
安装完成后在 Codex Desktop 中断开并重新连接这个 SSH 主机。

以后正常更新本机时，安装器会询问是否同步更新已登记的远程主机。也可以只更新远程：

```powershell
& $s -UpdateRemotes
```

```bash
# macOS 使用 install-macos.sh，Linux 使用 install-linux.sh
bash install-linux.sh --update-remotes
```

无人值守时可加 `-Yes` / `--yes`。离线或未带有 Codex DeepSeek 管理标记的主机会被
跳过，不会因为出现在 SSH 配置中就被修改；远程已是同一安装包版本时也不会重复安装。
同步远程版本时也会通过 SSH 加密通道复制本机由 `codex-dp provider` 管理的 Provider、
模型路由和密钥，并在远程重新应用。远程文件权限会设为 `0600`；这会让已登记服务器的
受管配置与本机保持一致，但不会修改不在该清单中的手工 Provider 配置。

需要恢复远程原有的 `codex` 入口时，在远程服务器执行：

```bash
curl -fsSL https://github.com/lyj96/codex-deepseek/releases/latest/download/install-linux.sh | bash -s -- --restore-ssh
```

## 使用

直接告诉主 Agent：

> 使用 model 为 deepseek-flash、思考程度 max 的子 Agent 检查这个项目并汇报结果。

支持 `low`、`high`、`max`。主 Agent 会使用 `spawn_external_agent`；后续可通过
`send_external_message` 和 `followup_external_task` 与同一个 DeepSeek 子 Agent 继续交互。

发送给 DeepSeek 子 Agent 的任务内容和必要上下文会通过 DeepSeek API 处理。

## 配置其他 Provider 和模型

DeepSeek 是默认预设；安装后也可以添加任意兼容 OpenAI Responses API 的 Provider。
直接运行交互式配置：

```bash
codex-dp provider add
```

也可以在运行一键安装脚本时加 `--configure-provider`；Windows 对应参数为
`-ConfigureProvider`。安装器会先保留默认 DeepSeek 配置，再进入相同的交互流程。

配置会依次询问 Provider 标识、名称、API 地址、密钥环境变量，并可连续添加多个模型。
每个模型可以使用独立的公开名称和 Provider 真实模型名，例如公开名称 `company-fast`
可以映射到 API 模型 `vendor/model-v3`，不再要求模型名以 Provider 名开头。

常用管理命令：

```bash
codex-dp provider list
codex-dp provider edit <provider-id> --base-url <url>
codex-dp provider model add <provider-id> <公开模型名> --api-model <API模型名>
codex-dp provider model edit <provider-id> <公开模型名> --reasoning-efforts low,high,max --default-reasoning-effort high
codex-dp provider model remove <provider-id> <公开模型名>
codex-dp provider remove <provider-id>
codex-dp provider validate
codex-dp provider test <provider-id>
```

`provider test` 检查本地清单、模型和密钥是否完整，不会发送或计费 API 请求；实际连通性
可在保存配置并重启 Codex 后拉起一次该模型的子 Agent 验证。

密钥单独保存在 `~/.codex/codex-dp/secrets.json`，不会写入 `config.toml`、Provider
清单或命令输出。Linux/macOS 上该文件权限为 `0600`。推荐先把 Key 放入临时环境变量，
再导入并立即清除临时变量：

```bash
codex-dp provider secret set COMPANY_API_KEY --from-env TEMP_COMPANY_KEY
```

也可以从标准输入导入：

```bash
printf '%s' "$TEMP_COMPANY_KEY" | codex-dp provider secret set COMPANY_API_KEY --stdin
```

Provider 清单位于 `~/.codex/codex-dp/providers.toml`；生成的模型目录和显式路由表位于
`~/.codex/model-catalogs/`。程序升级会保留这些文件，执行 `codex-dp provider apply`
即可重新生成 Codex 配置，并清理已经从受管清单中删除的 Provider 生成项。当前通用配置仅支持 Responses API；Chat Completions、Anthropic
Messages 等协议需要单独适配器，不能只修改 URL 直接使用。

## 支持的平台

| 系统                | Release 包                                                |
| ------------------- | --------------------------------------------------------- |
| Windows x64         | `codex-deepseek-package-x86_64-pc-windows-msvc.zip`       |
| Apple Silicon macOS | `codex-deepseek-package-aarch64-apple-darwin.tar.gz`      |
| Linux x64           | `codex-deepseek-package-x86_64-unknown-linux-musl.tar.gz` |

发布包没有商业代码签名。macOS 首次运行若被 Gatekeeper 拦截，请先核对 Release 中的
`SHA256SUMS`，再到“系统设置 → 隐私与安全性”允许运行。

## 官方 Codex 更新后怎么办

安装器把 Fork 放在独立目录，并让 Codex Desktop 通过稳定的 `CODEX_CLI_PATH` 使用它；
官方 App 或 CLI 更新不会覆盖这份程序。用户配置和模型目录也保存在 `~/.codex` 下。

但 Codex App 与 CLI 的内部协议可能随版本变化。我们会按上游 Codex 版本发布对应版本；
更新 Codex App 后如果出现不兼容，重新运行上面的一键安装命令即可升级到最新 Fork。
