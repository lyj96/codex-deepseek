"""Execution-platform identity for safe native V8 snapshot-generator reuse."""

V8SnapshotPlatformInfo = provider(fields = ["macos", "cpu"])

def _snapshot_platform_impl(ctx):
    cpu = ""
    if ctx.target_platform_has_constraint(ctx.attr._arm64[platform_common.ConstraintValueInfo]):
        cpu = "arm64"
    elif ctx.target_platform_has_constraint(ctx.attr._x64[platform_common.ConstraintValueInfo]):
        cpu = "x64"
    return [V8SnapshotPlatformInfo(
        macos = ctx.target_platform_has_constraint(ctx.attr._macos[platform_common.ConstraintValueInfo]),
        cpu = cpu,
    )]

v8_snapshot_platform = rule(
    implementation = _snapshot_platform_impl,
    attrs = {
        "_arm64": attr.label(default = "@platforms//cpu:aarch64"),
        "_macos": attr.label(default = "@platforms//os:macos"),
        "_x64": attr.label(default = "@platforms//cpu:x86_64"),
    },
)
