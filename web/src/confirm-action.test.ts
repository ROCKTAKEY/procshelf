import { test } from "node:test";
import assert from "node:assert/strict";
import { confirmAction, type Confirmation } from "./confirm-action.ts";

test("interrupting actions require approval naming the target and operation", async () => {
  for (const [action, label] of [
    ["stop", "停止"],
    ["restart", "再起動"],
    ["kill", "強制終了"],
  ] as const) {
    const prompts: Confirmation[] = [];
    assert.equal(
      await confirmAction(action, "API server", (message) => {
        prompts.push(message);
        return false;
      }),
      false,
    );
    assert.equal(prompts.length, 1);
    assert.equal(prompts[0].title, `API server を${label}しますか？`);
    assert.equal(prompts[0].label, `${label}する`);
    assert.ok(prompts[0].message.includes("配下"));
    assert.equal(await confirmAction(action, "API server", () => true), true);
  }
});
test("starting an idle process does not require confirmation", async () => {
  assert.equal(
    await confirmAction("start", "API server", () => {
      assert.fail("unexpected confirmation");
    }),
    true,
  );
});

test("waits for the modal decision before permitting an interruption", async () => {
  assert.equal(
    await confirmAction("stop", "API server", async () => false),
    false,
  );
});
