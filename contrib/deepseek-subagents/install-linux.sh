#!/usr/bin/env bash
set -euo pipefail

repo="lyj96/codex-deepseek"
asset="codex-deepseek-package-x86_64-unknown-linux-musl.tar.gz"
catalog_asset="deepseek-models.json"
install_dir="${HOME}/.local/share/codex-deepseek"
deepseek_key=""
release_tag="latest"

usage() {
  echo "Usage: install-linux.sh [--install-dir PATH] [--deepseek-key KEY] [--release TAG]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) install_dir="${2:?missing path after --install-dir}"; shift 2 ;;
    --deepseek-key) deepseek_key="${2:?missing key after --deepseek-key}"; shift 2 ;;
    --release) release_tag="${2:?missing tag after --release}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

[[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]] || {
  echo "This installer only supports Linux x86_64." >&2
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
  actual_hash="$(sha256sum "$temp_dir/$download" | awk '{print $1}')"
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

secret_dir="$HOME/.config/codex-deepseek"
secret_file="$secret_dir/env"
mkdir -p "$secret_dir"
chmod 700 "$secret_dir"
escaped_key="${deepseek_key//\'/\'\"\'\"\'}"
printf "DEEPSEEK_API_KEY='%s'\n" "$escaped_key" > "$secret_file"
chmod 600 "$secret_file"

wrapper="$install_dir/codex-deepseek"
{
  printf '%s\n' '#!/bin/sh'
  printf 'if [ -r %q ]; then . %q; fi\n' "$secret_file" "$secret_file"
  printf '%s\n' 'export DEEPSEEK_API_KEY'
  printf 'exec %q "$@"\n' "$current_dir/bin/codex"
} > "$wrapper"
chmod 700 "$wrapper"

bin_dir="$HOME/.local/bin"
mkdir -p "$bin_dir"
ln -sfn "$wrapper" "$bin_dir/codex-deepseek"
profile_path="$HOME/.profile"
touch "$profile_path"
if ! grep -Fq '# BEGIN CODEX DEEPSEEK' "$profile_path"; then
  [[ ! -s "$profile_path" ]] || cp "$profile_path" "$profile_path.bak.$(date +%Y%m%d%H%M%S)"
  escaped_wrapper="${wrapper//\'/\'\"\'\"\'}"
  {
    printf '\n%s\n' '# BEGIN CODEX DEEPSEEK'
    printf "export CODEX_CLI_PATH='%s'\n" "$escaped_wrapper"
    printf '%s\n' 'export PATH="$HOME/.local/bin:$PATH"'
    printf '%s\n' '# END CODEX DEEPSEEK'
  } >> "$profile_path"
fi
export CODEX_CLI_PATH="$wrapper"
export DEEPSEEK_API_KEY="$deepseek_key"

echo "Installed Codex DeepSeek to: $current_dir"
echo "CLI command: $bin_dir/codex-deepseek"
[[ -z "$backup_dir" ]] || echo "Previous installation kept at: $backup_dir"
echo "Open a new shell before using codex-deepseek."
