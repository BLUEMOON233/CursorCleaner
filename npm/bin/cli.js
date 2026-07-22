#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const supportedTargets = new Map([
  ["darwin-arm64", ["darwin-arm64", "cursor-cleaner"]],
  ["darwin-x64", ["darwin-x64", "cursor-cleaner"]],
  ["win32-x64", ["win32-x64", "cursor-cleaner.exe"]],
]);

const target = `${process.platform}-${process.arch}`;
const binaryParts = supportedTargets.get(target);

if (!binaryParts) {
  console.error(
    `cursor-cleaner does not support ${process.platform}/${process.arch}. ` +
      "Supported targets: macOS arm64, macOS x64, Windows x64.",
  );
  process.exit(1);
}

const packageRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const binary = path.join(packageRoot, "vendor", ...binaryParts);
const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: false,
});

if (result.error) {
  console.error(`Unable to start cursor-cleaner: ${result.error.message}`);
  process.exit(1);
}

if (result.signal) {
  console.error(`cursor-cleaner stopped by signal ${result.signal}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
