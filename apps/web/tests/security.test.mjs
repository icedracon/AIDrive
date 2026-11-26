import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

// SECURITY-REVIEW-2026-11 F1: the /api proxy must not be reachable from other
// computers, or a LAN client can forge Host and open the local owner session.
test("the web app listens on loopback only", () => {
  const { scripts } = JSON.parse(read("../package.json"));
  for (const name of ["dev", "start"]) {
    assert.match(scripts[name], /(^|\s)(-H|--hostname) 127\.0\.0\.1(\s|$)/, name);
  }
});

// SECURITY-REVIEW-2026-11 F4: release binaries are built and published by
// first-party actions only, and the checkout token is not left on disk.
test("release workflow uses first-party actions only", () => {
  const workflow = read("../../../.github/workflows/release.yml");
  const uses = [...workflow.matchAll(/^\s*-?\s*uses:\s*(\S+)/gm)].map((m) => m[1]);
  assert.ok(uses.length > 0);
  for (const action of uses) {
    assert.match(action, /^actions\//, action);
  }
  assert.match(workflow, /persist-credentials:\s*false/);
});
