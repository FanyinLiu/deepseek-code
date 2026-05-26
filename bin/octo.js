#!/usr/bin/env node
"use strict";

const path = require("path");
const { spawnSync } = require("child_process");
const { ensureCliBinary } = require("../scripts/npm-bootstrap");

(async () => {
  const result = await ensureCliBinary({ required: true });
  const binaryPath = result.path;
  if (!binaryPath) {
    console.error("[octo] No CLI binary is available");
    process.exit(1);
  }
  const args = process.argv.slice(2);

  const child = spawnSync(binaryPath, args, {
    stdio: "inherit",
    windowsHide: true,
    env: process.env,
  });

  if (child.error) {
    console.error(`[octo] Failed to execute binary: ${child.error.message}`);
    process.exit(1);
  }

  process.exit(child.status ?? 0);
})();
