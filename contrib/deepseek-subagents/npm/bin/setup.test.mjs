import assert from "node:assert/strict";
import test from "node:test";

import { releaseTagForVersion } from "./setup.js";

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
