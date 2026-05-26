"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");

const bootstrap = require("./npm-bootstrap");

test("target triple supports macOS arm64 and x64", () => {
  assert.equal(bootstrap.getTargetTriple("darwin", "arm64"), "aarch64-apple-darwin");
  assert.equal(bootstrap.getTargetTriple("darwin", "x64"), "x86_64-apple-darwin");
});

test("artifact naming uses version and target", () => {
  assert.equal(
    bootstrap.getReleaseArtifactName("1.2.3", "aarch64-apple-darwin"),
    "octo-v1.2.3-aarch64-apple-darwin.tar.gz"
  );
  assert.equal(
    bootstrap.getReleaseArtifactName("1.2.3", "x86_64-pc-windows-msvc"),
    "octo-v1.2.3-x86_64-pc-windows-msvc.zip"
  );
});

test("sha256 manifest parser accepts standard and binary markers", () => {
  const first = "a".repeat(64);
  const second = "b".repeat(64);
  const entries = bootstrap.parseSha256Manifest(`
${first}  octo-v1.0.0-aarch64-apple-darwin.tar.gz
${second} *octo-v1.0.0-x86_64-pc-windows-msvc.zip
`);

  assert.equal(entries.get("octo-v1.0.0-aarch64-apple-darwin.tar.gz"), first);
  assert.equal(entries.get("octo-v1.0.0-x86_64-pc-windows-msvc.zip"), second);
});

test("checksum mismatch throws before extraction", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "octo-bootstrap-test-"));
  try {
    const archive = path.join(dir, "octo-v1.0.0-aarch64-apple-darwin.tar.gz");
    fs.writeFileSync(archive, "not the expected artifact");
    await assert.rejects(
      () => bootstrap.verifyArtifactChecksum(archive, path.basename(archive), {
        expectedHash: "0".repeat(64),
      }),
      /Checksum mismatch/
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("dry-run output does not claim installation", () => {
  const script = path.join(__dirname, "npm-bootstrap.js");
  const output = spawnSync(process.execPath, [script, "--dry-run"], {
    encoding: "utf8",
  });

  assert.equal(output.status, 0, output.stderr);
  assert.match(output.stdout, /Dry run:/);
  assert.doesNotMatch(output.stdout, /CLI installed/);
});
