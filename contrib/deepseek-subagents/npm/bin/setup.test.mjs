import assert from "node:assert/strict";
import test from "node:test";

import { releaseTagForVersion, windowsPowerShellEnv } from "./setup.js";

test("normal npm versions select the matching GitHub Release", () => {
  assert.equal(
    releaseTagForVersion("0.154.0-deepseek.2"),
    "codex-v0.154.0-deepseek.2",
  );
});

test("npm-only hotfix versions keep the native GitHub Release", () => {
  assert.equal(
    releaseTagForVersion("0.154.0-deepseek.2-npm.1"),
    "codex-v0.154.0-deepseek.2",
  );
});

test("Windows PowerShell does not inherit a PowerShell 7 module path", () => {
  const original = {
    Path: "C:\\Windows\\System32",
    PSModulePath: "C:\\Program Files\\PowerShell\\Modules",
    pSmOdUlEpAtH: "C:\\another-conflicting-module-path",
    CODEX_HOME: "C:\\codex-home",
  };

  assert.deepEqual(windowsPowerShellEnv(original), {
    Path: original.Path,
    CODEX_HOME: original.CODEX_HOME,
  });
  assert.equal(original.PSModulePath, "C:\\Program Files\\PowerShell\\Modules");
});
