import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("launcher declares every supported native target", () => {
  const launcher = readFileSync(new URL("../bin/cli.js", import.meta.url), "utf8");
  for (const target of ["darwin-arm64", "darwin-x64", "win32-x64"]) {
    assert.match(launcher, new RegExp(`\\[\\"${target}\\"`));
  }
});

test("package has no install lifecycle scripts", () => {
  const pkg = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  );
  assert.equal(pkg.scripts, undefined);
});
