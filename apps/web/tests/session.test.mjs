import assert from "node:assert/strict";
import test from "node:test";
import { requestSignOut } from "../src/lib/session.mjs";

test("logout requires explicit revocation acknowledgement", async () => {
  assert.equal(await requestSignOut(async () => new Response(null, { status: 204 })), true);
});

for (const status of [200, 401, 403, 500, 503]) {
  test(`HTTP ${status} does not confirm logout`, async () => {
    assert.equal(await requestSignOut(async () => new Response("unexpected", { status })), false);
  });
}

test("a lost connection leaves logout unconfirmed", async () => {
  assert.equal(await requestSignOut(async () => { throw new TypeError("Failed to fetch"); }), false);
});

test("failed logout can be retried", async () => {
  let attempts = 0;
  const request = async () => new Response(null, { status: ++attempts === 1 ? 503 : 204 });
  assert.equal(await requestSignOut(request), false);
  assert.equal(await requestSignOut(request), true);
});
