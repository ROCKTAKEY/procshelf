import { readFile, writeFile } from "node:fs/promises";

const check = process.argv.includes("--check");
const manifest = await readFile("Cargo.toml", "utf8");
const version = manifest
  .match(/^\[package\]\s*\n([\s\S]*?)(?=^\[|(?![\s\S]))/m)?.[1]
  .match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
if (!version)
  throw new Error("Cargo.toml must declare the Procshelf release version");

const cargo = await readFile("Cargo.lock", "utf8");
const entry = /(\[\[package\]\]\nname = "procshelf"\nversion = ")[^"]+(")/;
if (!entry.test(cargo)) throw new Error("Cargo.lock has no Procshelf package");
const pkg = JSON.parse(await readFile("web/package.json", "utf8"));
const lock = JSON.parse(await readFile("web/package-lock.json", "utf8"));
if (
  pkg.name !== "procshelf-web" ||
  lock.name !== pkg.name ||
  !lock.packages?.[""]
) {
  throw new Error("Unexpected web package metadata");
}

if (check) {
  if (
    !cargo.includes('name = "procshelf"\nversion = "' + version + '"') ||
    pkg.version !== version ||
    lock.version !== version ||
    lock.packages[""].version !== version
  ) {
    console.error(
      "Release versions differ; run ./scripts/dev node scripts/sync-versions.mjs",
    );
    process.exitCode = 1;
  }
} else {
  pkg.version = version;
  lock.version = version;
  lock.packages[""].version = version;
  await writeFile("Cargo.lock", cargo.replace(entry, `$1${version}$2`));
  await writeFile("web/package.json", JSON.stringify(pkg, null, 2) + "\n");
  await writeFile(
    "web/package-lock.json",
    JSON.stringify(lock, null, 2) + "\n",
  );
}
