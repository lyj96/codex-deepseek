#!/usr/bin/env bash
set -euo pipefail

repo="lyj96/codex-deepseek"
asset="codex-deepseek-package-aarch64-apple-darwin.tar.gz"
catalog_asset="deepseek-models.json"
install_dir="${HOME}/Library/Application Support/CodexDeepSeek"
deepseek_key="${DEEPSEEK_API_KEY:-}"
release_tag="latest"
ssh_host=""
discover_ssh=false
update_remotes_only=false
assume_yes=false
registry_dir="${XDG_CONFIG_HOME:-$HOME/.config}/codex-deepseek"
managed_hosts_file="$registry_dir/ssh-hosts"

usage() {
  cat <<'EOF'
Usage: install-macos.sh [--install-dir PATH] [--deepseek-key KEY] [--release TAG]
                        [--ssh-host HOST | --discover-ssh | --update-remotes]
                        [--yes]

Remote host options currently support Linux x86_64 SSH targets.
EOF
}

validate_release_tag() {
  [[ "$release_tag" == "latest" || "$release_tag" =~ ^codex-v[0-9]+\.[0-9]+\.[0-9]+-deepseek\.[1-9][0-9]*$ ]] || {
    echo "Invalid release tag: $release_tag" >&2
    exit 2
  }
}

validate_ssh_host() {
  local host="$1"
  [[ -n "$host" && "$host" != -* && "$host" =~ ^[A-Za-z0-9_.@:-]+$ ]] || {
    echo "Invalid SSH host or alias: $host" >&2
    exit 2
  }
}

remote_installer_url() {
  if [[ "$release_tag" == "latest" ]]; then
    printf 'https://github.com/%s/releases/latest/download/install-linux.sh\n' "$repo"
  else
    printf 'https://github.com/%s/releases/download/%s/install-linux.sh\n' "$repo" "$release_tag"
  fi
}

register_managed_host() {
  local host="$1"
  mkdir -p "$registry_dir"
  touch "$managed_hosts_file"
  grep -Fqx "$host" "$managed_hosts_file" || printf '%s\n' "$host" >> "$managed_hosts_file"
}

list_discovered_hosts() {
  local ssh_config="$HOME/.ssh/config"
  [[ -r "$ssh_config" ]] || {
    echo "No readable SSH config found at: $ssh_config"
    return
  }
  echo "SSH host candidates from $ssh_config:"
  awk '
    tolower($1) == "host" {
      for (i = 2; i <= NF; i++) {
        if ($i !~ /[*!?]/ && $i !~ /^!/) print $i
      }
    }
  ' "$ssh_config" | sort -u | while IFS= read -r host; do
    if [[ -r "$managed_hosts_file" ]] && grep -Fqx "$host" "$managed_hosts_file"; then
      printf '  %s (managed)\n' "$host"
    else
      printf '  %s\n' "$host"
    fi
  done
  echo "Install and register one with: --ssh-host HOST"
}

load_local_deepseek_key() {
  if [[ -z "$deepseek_key" ]]; then
    deepseek_key="$(security find-generic-password -a "$USER" -s codex-deepseek-api-key -w 2>/dev/null || true)"
  fi
  [[ "$deepseek_key" != *$'\n'* && "$deepseek_key" != *$'\r'* ]] || {
    echo "DeepSeek API Key cannot contain a newline." >&2
    exit 1
  }
}

install_remote_host() {
  local host="$1"
  local forward_key="${2:-}"
  local installer_url url_q tag_q remote_command
  validate_ssh_host "$host"
  command -v ssh >/dev/null 2>&1 || { echo "OpenSSH client 'ssh' is required." >&2; return 1; }
  installer_url="$(remote_installer_url)"
  printf -v url_q '%q' "$installer_url"
  printf -v tag_q '%q' "$release_tag"
  remote_command="set -eu; platform=\$(uname -s)/\$(uname -m); if [ \"\$platform\" != Linux/x86_64 ]; then echo \"Unsupported remote platform: \$platform (expected Linux/x86_64)\" >&2; exit 1; fi; tmp=\$(mktemp \"\${TMPDIR:-/tmp}/codex-deepseek-installer.XXXXXX\"); trap 'rm -f \"\$tmp\"' EXIT; curl -fL --retry 3 $url_q -o \"\$tmp\"; chmod 700 \"\$tmp\"; bash \"\$tmp\" --ssh-remote --release $tag_q"
  echo "Installing Codex DeepSeek on SSH host: $host"
  if [[ -n "$forward_key" ]]; then
    remote_command="IFS= read -r DEEPSEEK_API_KEY; DEEPSEEK_API_KEY=\$(printf '%s' \"\$DEEPSEEK_API_KEY\" | tr -d '\\r'); export DEEPSEEK_API_KEY; $remote_command"
    printf '%s\n' "$forward_key" | ssh -- "$host" "$remote_command"
  else
    ssh -t -- "$host" "$remote_command"
  fi
  register_managed_host "$host"
  echo "Registered managed SSH host: $host"
}

update_managed_remotes() {
  local desired_version="${1:-}"
  local host remote_version found=false failed=false
  [[ -s "$managed_hosts_file" ]] || {
    echo "No managed SSH hosts. Register one with --ssh-host HOST."
    return 0
  }
  while IFS= read -r host; do
    [[ -n "$host" ]] || continue
    found=true
    if remote_version="$(ssh -o BatchMode=yes -o ConnectTimeout=8 -- "$host" 'test -f "$HOME/.config/codex-deepseek/ssh-managed" && sed -n "s/^package_version=//p" "$HOME/.config/codex-deepseek/ssh-managed"' 2>/dev/null)"; then
      if [[ -n "$desired_version" && "$remote_version" == "$desired_version" ]]; then
        echo "Already current on SSH host: $host ($desired_version)"
        continue
      fi
      install_remote_host "$host" || failed=true
    else
      echo "Skipping unavailable or unmanaged SSH host: $host" >&2
      failed=true
    fi
  done < "$managed_hosts_file"
  [[ "$found" == true ]] || echo "No managed SSH hosts."
  [[ "$failed" == false ]]
}

offer_remote_updates() {
  local desired_version="${1:-}"
  [[ -s "$managed_hosts_file" ]] || return 0
  if [[ "$assume_yes" == true ]]; then
    update_managed_remotes "$desired_version" || echo "One or more managed SSH hosts could not be updated." >&2
    return
  fi
  local answer=""
  if [[ -r /dev/tty ]]; then
    read -r -p "Update registered SSH hosts with this release? [y/N] " answer </dev/tty
  fi
  if [[ "$answer" =~ ^[Yy]([Ee][Ss])?$ ]]; then
    update_managed_remotes "$desired_version" || echo "One or more managed SSH hosts could not be updated." >&2
  else
    echo "Remote hosts were not changed. Run again with --update-remotes when ready."
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) install_dir="${2:?missing path after --install-dir}"; shift 2 ;;
    --deepseek-key) deepseek_key="${2:?missing key after --deepseek-key}"; shift 2 ;;
    --release) release_tag="${2:?missing tag after --release}"; shift 2 ;;
    --ssh-host) ssh_host="${2:?missing host after --ssh-host}"; shift 2 ;;
    --discover-ssh) discover_ssh=true; shift ;;
    --update-remotes) update_remotes_only=true; shift ;;
    --yes) assume_yes=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

validate_release_tag
mode_count=0
[[ "$discover_ssh" == false ]] || mode_count=$((mode_count + 1))
[[ -z "$ssh_host" ]] || mode_count=$((mode_count + 1))
[[ "$update_remotes_only" == false ]] || mode_count=$((mode_count + 1))
[[ "$mode_count" -le 1 ]] || { echo "Choose only one SSH operation at a time." >&2; exit 2; }
if [[ "$discover_ssh" == true ]]; then
  list_discovered_hosts
  exit 0
fi
if [[ -n "$ssh_host" ]]; then
  load_local_deepseek_key
  install_remote_host "$ssh_host" "$deepseek_key"
  exit 0
fi
if [[ "$update_remotes_only" == true ]]; then
  update_managed_remotes
  exit 0
fi

[[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] || {
  echo "This installer only supports Apple Silicon macOS." >&2
  exit 1
}
[[ "$install_dir" != *$'\n'* && "$install_dir" != *$'\r'* ]] || {
  echo "Install path cannot contain a newline." >&2
  exit 1
}
[[ "$install_dir" == /* ]] || install_dir="$PWD/$install_dir"
install_dir="${install_dir%/}"
[[ -n "$install_dir" && "$install_dir" != "/" ]] || {
  echo "Install path cannot be the filesystem root." >&2
  exit 1
}
load_local_deepseek_key
if [[ -z "$deepseek_key" ]]; then
  read -r -s -p "DeepSeek API Key: " deepseek_key </dev/tty
  echo
fi
[[ -n "$deepseek_key" ]] || { echo "DeepSeek API Key cannot be empty." >&2; exit 1; }
[[ "$deepseek_key" != *$'\n'* && "$deepseek_key" != *$'\r'* ]] || {
  echo "DeepSeek API Key cannot contain a newline." >&2
  exit 1
}

if [[ "$release_tag" == "latest" ]]; then
  download_root="https://github.com/${repo}/releases/latest/download"
else
  download_root="https://github.com/${repo}/releases/download/${release_tag}"
fi

temp_root="${TMPDIR:-/tmp}"
temp_root="${temp_root%/}"
temp_dir="$(mktemp -d "$temp_root/codex-deepseek.XXXXXX")"
cleanup() {
  case "$temp_dir" in
    "$temp_root"/codex-deepseek.*) rm -rf -- "$temp_dir" ;;
  esac
}
trap cleanup EXIT
mkdir -p "$temp_dir/package"
curl -fL --retry 3 "$download_root/$asset" -o "$temp_dir/$asset"
curl -fL --retry 3 "$download_root/$catalog_asset" -o "$temp_dir/$catalog_asset"
curl -fL --retry 3 "$download_root/SHA256SUMS" -o "$temp_dir/SHA256SUMS"
for download in "$asset" "$catalog_asset"; do
  expected_hash="$(awk -v name="$download" '$2 == name || $2 == "*" name { print $1; exit }' "$temp_dir/SHA256SUMS")"
  [[ -n "$expected_hash" ]] || { echo "SHA256SUMS does not contain $download." >&2; exit 1; }
  actual_hash="$(shasum -a 256 "$temp_dir/$download" | awk '{print $1}')"
  [[ "$actual_hash" == "$expected_hash" ]] || { echo "SHA-256 verification failed for $download." >&2; exit 1; }
done
tar -xzf "$temp_dir/$asset" -C "$temp_dir/package"
[[ -x "$temp_dir/package/bin/codex" ]] || {
  echo "Downloaded package does not contain bin/codex." >&2
  exit 1
}
"$temp_dir/package/bin/codex" --version

mkdir -p "$install_dir"
current_dir="$install_dir/current"
backup_dir=""
if [[ -e "$current_dir" ]]; then
  backup_dir="$install_dir/previous-$(date +%Y%m%d%H%M%S)-$$"
  mv "$current_dir" "$backup_dir"
fi
if ! mv "$temp_dir/package" "$current_dir"; then
  [[ -n "$backup_dir" && ! -e "$current_dir" ]] && mv "$backup_dir" "$current_dir"
  exit 1
fi
package_version="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$current_dir/codex-package.json" | head -n1)"
[[ -n "$package_version" ]] || { echo "Installed package metadata does not contain a version." >&2; exit 1; }

codex_home="${CODEX_HOME:-$HOME/.codex}"
catalog_dir="$codex_home/model-catalogs"
mkdir -p "$catalog_dir"
cp "$temp_dir/$catalog_asset" "$catalog_dir/deepseek.json"

config_path="$codex_home/config.toml"
touch "$config_path"
if ! grep -Eq '^[[:space:]]*\[model_providers\.deepseek\][[:space:]]*(#.*)?$' "$config_path"; then
  [[ ! -s "$config_path" ]] || cp "$config_path" "$config_path.bak.$(date +%Y%m%d%H%M%S)"
  printf '\n%s\n' '[model_providers.deepseek]' >> "$config_path"
  printf '%s\n' 'name = "DeepSeek"' >> "$config_path"
  printf '%s\n' 'base_url = "https://api.deepseek.com/"' >> "$config_path"
  printf '%s\n' 'env_key = "DEEPSEEK_API_KEY"' >> "$config_path"
  printf '%s\n' 'wire_api = "responses"' >> "$config_path"
  printf '%s\n' 'supports_websockets = false' >> "$config_path"
fi

"$current_dir/bin/codex" features enable multi_agent_v2

keychain_service="codex-deepseek-api-key"
security add-generic-password -U -a "$USER" -s "$keychain_service" -w "$deepseek_key" -T /usr/bin/security >/dev/null
wrapper="$install_dir/codex-dp"
{
  printf '%s\n' '#!/bin/sh'
  printf '%s\n' 'DEEPSEEK_API_KEY="$(/usr/bin/security find-generic-password -a "$USER" -s codex-deepseek-api-key -w)"'
  printf '%s\n' 'export DEEPSEEK_API_KEY'
  printf 'exec %q "$@"\n' "$current_dir/bin/codex"
} > "$wrapper"
chmod 700 "$wrapper"

bin_dir="$HOME/.local/bin"
mkdir -p "$bin_dir"
primary_launcher="$bin_dir/codex-dp"
legacy_launcher="$bin_dir/codex-deepseek"
if [[ ! -e "$primary_launcher" && ! -L "$primary_launcher" ]] ||
   [[ -L "$primary_launcher" && ("$(readlink "$primary_launcher")" == "$wrapper" || "$(readlink "$primary_launcher")" == "$install_dir/codex-deepseek") ]]; then
  ln -sfn "$wrapper" "$primary_launcher"
else
  echo "Keeping existing codex-dp launcher: $primary_launcher"
fi
# Compatibility for installations made before the CLI name was unified.
ln -sfn "$wrapper" "$legacy_launcher"
profile_path="$HOME/.zprofile"
touch "$profile_path"
if ! grep -Fq '# BEGIN CODEX DEEPSEEK' "$profile_path"; then
  [[ ! -s "$profile_path" ]] || cp "$profile_path" "$profile_path.bak.$(date +%Y%m%d%H%M%S)"
  {
    printf '\n%s\n' '# BEGIN CODEX DEEPSEEK'
    printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"'
    printf '%s\n' '# END CODEX DEEPSEEK'
  } >> "$profile_path"
fi

launchctl setenv CODEX_CLI_PATH "$wrapper"
launch_agent_dir="$HOME/Library/LaunchAgents"
launch_agent="$launch_agent_dir/com.lyj96.codex-deepseek-env.plist"
mkdir -p "$launch_agent_dir"
xml_wrapper="$(printf '%s' "$wrapper" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g')"
{
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>'
  printf '%s\n' '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
  printf '%s\n' '<plist version="1.0"><dict>'
  printf '%s\n' '<key>Label</key><string>com.lyj96.codex-deepseek-env</string>'
  printf '%s\n' '<key>ProgramArguments</key><array>'
  printf '%s\n' '<string>/bin/launchctl</string><string>setenv</string><string>CODEX_CLI_PATH</string>'
  printf '<string>%s</string>\n' "$xml_wrapper"
  printf '%s\n' '</array><key>RunAtLoad</key><true/></dict></plist>'
} > "$launch_agent"
launchctl bootout "gui/$(id -u)" "$launch_agent" >/dev/null 2>&1 || true
launchctl bootstrap "gui/$(id -u)" "$launch_agent"

echo "Installed Codex DeepSeek to: $current_dir"
echo "CLI command: codex-dp"
[[ -z "$backup_dir" ]] || echo "Previous installation kept at: $backup_dir"
echo "Fully quit and reopen Codex Desktop before using DeepSeek subagents."
offer_remote_updates "$package_version"
