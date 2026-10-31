import { test } from "node:test";
import assert from "node:assert/strict";
import { parseScreen, screenHash } from "./navigation.ts";

test("a process URL opens its workspace, where information can expand without navigation", () => {
  const screen = { kind: "process", id: "worker-1" } as const;
  assert.deepEqual(parseScreen("#/processes/worker-1"), screen);
  assert.equal(screenHash(screen), "#/processes/worker-1");
});
test("malformed and unsupported URLs return to the list", () => {
  for (const hash of [
    "",
    "#/",
    "#/processes/%ZZ",
    "#/processes/a%2Fb",
    "#/processes/a/kill",
    "#/other",
  ])
    assert.deepEqual(parseScreen(hash), { kind: "list" });
});
