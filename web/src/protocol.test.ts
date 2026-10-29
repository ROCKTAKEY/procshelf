import { test } from "node:test";
import assert from "node:assert/strict";
import { snapshotSchema, canStart } from "./protocol.ts";

test("rejects incomplete server state instead of enabling controls on unvalidated input", () => {
  assert.equal(snapshotSchema.safeParse({ processes: [] }).success, false);
});
test("only registered idle processes can start", () => {
  for (const state of ["stopped", "exited", "failed"])
    assert.equal(canStart(state, false), true);
  for (const state of ["starting", "running", "stopping", "unknown"])
    assert.equal(canStart(state, false), false);
  assert.equal(canStart("stopped", true), false);
});
