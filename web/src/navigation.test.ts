import { test } from "node:test";
import assert from "node:assert/strict";
import { parseScreen, screenHash, parentScreen } from "./navigation.ts";

test("a shared URL selects the same process and information screen", () => {
  const screen = {
    kind: "process",
    id: "worker-1",
    panel: "information",
  } as const;
  assert.deepEqual(parseScreen(screenHash(screen)), screen);
});
test("malformed or unsupported URLs return to the list", () => {
  for (const hash of [
    "",
    "#/",
    "#/processes/%ZZ/operations",
    "#/processes/a%2Fb/operations",
    "#/processes/a/kill",
    "#/other",
  ])
    assert.deepEqual(parseScreen(hash), { kind: "list" });
});
test("information returns to the same process before returning to the list", () => {
  const information = {
    kind: "process",
    id: "clock",
    panel: "information",
  } as const;
  const operations = parentScreen(information);
  assert.deepEqual(operations, {
    kind: "process",
    id: "clock",
    panel: "operations",
  });
  assert.deepEqual(parentScreen(operations), { kind: "list" });
});
