import { test } from "node:test";
import assert from "node:assert/strict";
import { confirmAction } from "./confirm-action.ts";

test("interrupting actions require approval naming the target and operation", () => {
  for (const [action, label] of [
    ["stop", "停止"],
    ["restart", "再起動"],
    ["kill", "強制終了"],
  ] as const) {
    const prompts: string[] = [];
    assert.equal(
      confirmAction(action, "API server", (message) => {
        prompts.push(message);
        return false;
      }),
      false,
    );
    assert.equal(prompts.length, 1);
    assert.ok(prompts[0].includes("API server"));
    assert.ok(prompts[0].includes(label));
    assert.equal(
      confirmAction(action, "API server", () => true),
      true,
    );
  }
});
test("starting an idle process does not require confirmation", () => {
  assert.equal(
    confirmAction("start", "API server", () => {
      assert.fail("unexpected confirmation");
    }),
    true,
  );
});
