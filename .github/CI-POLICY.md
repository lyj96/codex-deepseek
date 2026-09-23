# Fork CI and release gates

The fork ships Cargo-built Windows x86_64 MSVC, macOS arm64 and Linux x86_64
musl packages. `blocking-ci` is the automatic PR gate. It retains formatting,
dependency policy, repository checks, Python SDK installation, provider-manager
and installer/npm tests, App Server account/model/settings tests, and Cargo
multi-agent/provider/agent-control regressions. Windows MSVC and Apple Silicon
macOS compile checks cover the release binaries to catch platform-only Rust
errors before release. Skipped or failed required workflows are not accepted by
the final gate.

Native releases run that same gate on the tagged revision. Publishing requires
both validation and all three Cargo package builds (including executable version
smoke tests). npm-only wrapper hotfixes keep their no-Rust-build path.

Before creating a native release tag, verify the exact candidate revision locally
with the pinned Rust toolchain: compile the Windows MSVC release entrypoints,
link and launch `codex.exe`, and compile plus launch the CLI under WSL/Linux.
Use the checksum-verified V8 artifacts, not the crate's unsupported default
download URL. The latest PR revision must also pass `blocking-ci`. If either
platform preflight or the PR gate fails, fix it before tagging; a release build
must not be used as the first platform-specific compilation test.

`extended-ci` is manually dispatched on a reviewed ref. It preserves the full
Bazel matrix, Clippy, SDK integration tests and argument-comment lint. These
are supplemental signals, not prerequisites for ordinary Cargo releases.
Failures remain failures; they are not relabeled successful.
The upstream V8 canary matrix is also manual on this fork; it is not started
automatically by a PR.

## Coverage not equivalent to the upstream suite

The required Cargo regressions are focused, not the entire upstream test suite.
In particular, Bazel runfiles/toolchain integration, GNU Windows builds, the
full cross-platform unit/integration matrix, full SDK integration, Clippy and
argument-comment lint remain extended coverage. This is an explicit coverage
tradeoff, not a claim that Cargo package compilation replaces those tests.

Dispatch extended checks when updating upstream build infrastructure, Bazel
rules/lockfiles, native dependencies, V8 or SDKs, or investigating a failure in
those areas. For provider/agent changes, first expand the focused Cargo tests
to cover the behavior; existing filters alone do not prove compatibility.
Known optional infrastructure failures do not waive a reproduced product bug.

Use standard free runners. Do not enable paid runners, delete tests, or change
required checks merely to make a failing run green. Cancel superseded PR runs;
retain failure evidence. Clean local build caches after verification, preserving
source, configuration, secrets and the latest verified installed executable.
