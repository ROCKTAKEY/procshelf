import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const script = new URL("./sync-versions.mjs", import.meta.url);
test("release versions stay aligned without changing dependency versions", async () => {
  const root = await mkdtemp(join(tmpdir(), "procshelf-version-"));
  try {
    await mkdir(join(root, "web"));
    await writeFile(
      join(root, "Cargo.toml"),
      '[package]\nname = "procshelf"\nversion = "0.2.0"\n',
    );
    await writeFile(
      join(root, "Cargo.lock"),
      'version = 4\n\n[[package]]\nname = "dependency"\nversion = "0.1.1"\n\n[[package]]\nname = "procshelf"\nversion = "0.1.1"\n',
    );
    await writeFile(
      join(root, "web/package.json"),
      JSON.stringify({
        name: "procshelf-web",
        version: "0.1.1",
        dependencies: { example: "0.1.1" },
      }),
    );
    await writeFile(
      join(root, "web/package-lock.json"),
      JSON.stringify({
        name: "procshelf-web",
        version: "0.1.1",
        packages: {
          "": { version: "0.1.1" },
          "node_modules/example": { version: "0.1.1" },
        },
      }),
    );
    const run = (...args) =>
      spawnSync(process.execPath, [script.pathname, ...args], {
        cwd: root,
        encoding: "utf8",
      });
    assert.equal(run("--check").status, 1);
    const synced = run();
    assert.equal(synced.status, 0, synced.stderr);
    assert.equal(run("--check").status, 0);
    const cargo = await readFile(join(root, "Cargo.lock"), "utf8");
    assert.match(cargo, /name = "dependency"\nversion = "0.1.1"/);
    assert.match(cargo, /name = "procshelf"\nversion = "0.2.0"/);
    const pkg = JSON.parse(await readFile(join(root, "web/package.json")));
    assert.equal(pkg.version, "0.2.0");
    assert.equal(pkg.dependencies.example, "0.1.1");
    const lock = JSON.parse(
      await readFile(join(root, "web/package-lock.json")),
    );
    assert.equal(lock.version, "0.2.0");
    assert.equal(lock.packages[""].version, "0.2.0");
    assert.equal(lock.packages["node_modules/example"].version, "0.1.1");
    const before = await readFile(join(root, "web/package-lock.json"), "utf8");
    assert.equal(run().status, 0);
    assert.equal(
      await readFile(join(root, "web/package-lock.json"), "utf8"),
      before,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
