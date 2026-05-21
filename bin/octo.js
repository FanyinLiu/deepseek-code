#!/usr/bin/env node
"use strict";

const path = require("path");
const { spawnSync } = require("child_process");
const { ensureCliBinary } = require("../scripts/npm-bootstrap");

(async () => {
  const binaryPath = await ensureCliBinary({ required: true });
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
