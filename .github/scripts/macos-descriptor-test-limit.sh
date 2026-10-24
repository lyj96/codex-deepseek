#!/usr/bin/env bash
set -euo pipefail

# This fixture needs a valid fd >= Darwin OPEN_MAX. XNU clamps RLIMIT_NOFILE
# to kern.maxfilesperproc, even after the private test child raises its rlimit.
# Only prepare disposable hosted CI; never change a developer's Mac.
[[ "${GITHUB_ACTIONS:-}" == true && "${RUNNER_OS:-}" == macOS && "${CODEX_DESCRIPTOR_TEST_HOSTED:-}" == github-hosted ]] || {
  printf 'Descriptor resource setup requires a disposable GitHub-hosted macOS runner\n' >&2
  exit 1
}

current="$(/usr/sbin/sysctl -n kern.maxfilesperproc)"
[[ "$current" =~ ^[0-9]+$ ]]
needed=16384
case "${1:?prepare or restore required}" in
  prepare)
    : "${GITHUB_ENV:?GitHub environment file required}"
    printf 'CODEX_DESCRIPTOR_TEST_ORIGINAL_MAXFILES=%s\n' "$current" >> "$GITHUB_ENV"
    printf 'Descriptor fixture kernel ceiling before=%s required=%s\n' "$current" "$needed"
    if (( current < needed )); then
      sudo -n /usr/sbin/sysctl -w "kern.maxfilesperproc=$needed"
    fi
    actual="$(/usr/sbin/sysctl -n kern.maxfilesperproc)"
    [[ "$actual" =~ ^[0-9]+$ ]]
    (( actual >= needed ))
    printf 'Descriptor fixture kernel ceiling prepared=%s\n' "$actual"
    ;;
  restore)
    original="${CODEX_DESCRIPTOR_TEST_ORIGINAL_MAXFILES:?Original ceiling missing}"
    [[ "$original" =~ ^[0-9]+$ ]]
    if (( current != original )); then
      (( current == needed )) || { printf 'Unexpected kernel ceiling; do not overwrite it\n' >&2; exit 1; }
      sudo -n /usr/sbin/sysctl -w "kern.maxfilesperproc=$original"
    fi
    [[ "$(/usr/sbin/sysctl -n kern.maxfilesperproc)" == "$original" ]]
    printf 'Descriptor fixture kernel ceiling restored=%s\n' "$original"
    ;;
  *) printf 'Expected prepare or restore\n' >&2; exit 1 ;;
esac
