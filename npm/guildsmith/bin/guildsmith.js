#!/usr/bin/env node
// Launcher for the guildsmith binary (research R15, ADR-002).
//
// npm installs exactly one @guildsmith/<platform> package through optionalDependencies, each
// limited by `os` and `cpu`. This script finds it and runs its binary with the same arguments
// and stdio, then exits with the binary's exit code. There is no postinstall step, so it works
// with `--ignore-scripts` and pnpm's script blocking.
"use strict";

const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const PLATFORMS = {
  "linux-x64": "@guildsmith/linux-x64",
  "linux-arm64": "@guildsmith/linux-arm64",
  "darwin-x64": "@guildsmith/darwin-x64",
  "darwin-arm64": "@guildsmith/darwin-arm64",
  "win32-x64": "@guildsmith/win32-x64",
};

function binaryPath() {
  // Escape hatch for testing a local build: GUILDSMITH_BINARY=/path/to/guildsmith.
  if (process.env.GUILDSMITH_BINARY) {
    return process.env.GUILDSMITH_BINARY;
  }
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORMS[key];
  if (!pkg) {
    fail(
      `guildsmith has no prebuilt binary for ${key}. ` +
        `Supported: ${Object.keys(PLATFORMS).join(", ")}. ` +
        "Install from source with `cargo install guildsmith`.",
    );
  }
  let dir;
  try {
    dir = path.dirname(require.resolve(`${pkg}/package.json`));
  } catch {
    fail(
      `the package ${pkg} is not installed. It is an optional dependency of guildsmith; ` +
        "reinstall without --no-optional / --omit=optional.",
    );
  }
  const exe = process.platform === "win32" ? "guildsmith.exe" : "guildsmith";
  return path.join(dir, "bin", exe);
}

function fail(message) {
  process.stderr.write(`guildsmith: ${message}\n`);
  process.exit(1);
}

const bin = binaryPath();
if (process.platform !== "win32") {
  // Some package managers drop the executable bit; restore it when we can.
  try {
    fs.accessSync(bin, fs.constants.X_OK);
  } catch {
    try {
      fs.chmodSync(bin, 0o755);
    } catch {
      // Read-only install: let the spawn below report the real error.
    }
  }
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
if (result.error) {
  fail(`can't run ${bin}: ${result.error.message}`);
}
if (result.signal) {
  // Die the same way the binary did, so callers see the signal.
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status === null ? 1 : result.status);
}
