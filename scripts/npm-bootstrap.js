#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const crypto = require("node:crypto");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const pkg = require("../package.json");

const PACKAGE_ROOT = path.resolve(__dirname, "..");
const REPO_ORIGIN = (pkg.repository && pkg.repository.url || "https://github.com/FanyinLiu/octo")
  .replace(/^git\+/, "")
  .replace(/\/$/, "")
  .replace(/\.git$/, "");
const DEFAULT_VERSION = String(pkg.version || "0.0.0");
const WINDOWS_EXE = process.platform === "win32" ? ".exe" : "";

function runCommand(command, args) {
  const child = spawnSync(command, args, { stdio: "inherit" });
  if (child.error) {
    throw child.error;
  }
  if (child.status !== 0) {
    throw new Error(`command failed: ${command} ${args.join(" ")}`);
  }
}

function getInstallVersion() {
  return (process.env.OCTO_VERSION || DEFAULT_VERSION).replace(/^v/, "");
}

function getTargetTriple(platform = process.platform, arch = process.arch) {
  if (platform === "win32") {
    if (arch === "x64") return "x86_64-pc-windows-msvc";
    throw new Error(`Unsupported Windows architecture: ${arch}`);
  }

  if (platform === "darwin") {
    if (arch === "arm64") return "aarch64-apple-darwin";
    if (arch === "x64") return "x86_64-apple-darwin";
    throw new Error(`Unsupported macOS architecture: ${arch}`);
  }

  if (platform === "linux") {
    if (arch === "x64") return "x86_64-unknown-linux-gnu";
    throw new Error(`Unsupported Linux architecture: ${arch}`);
  }

  throw new Error(`Unsupported platform: ${platform}`);
}

function getReleaseArtifactName(version, target) {
  const ext = target.includes("windows") ? "zip" : "tar.gz";
  return `octo-v${version}-${target}.${ext}`;
}

function getCachedBinaryPath(version, target) {
  const binDir = path.join(os.homedir(), ".octo", version, target);
  const binary = `octo${WINDOWS_EXE}`;
  return path.join(binDir, binary);
}

function getLocalSourceBinaryPath() {
  if (process.env.OCTO_BOOTSTRAP_FORCE_DOWNLOAD === "1") {
    return null;
  }

  const cargoManifest = path.join(PACKAGE_ROOT, "Cargo.toml");
  const sourceEntry = path.join(PACKAGE_ROOT, "src", "cli_entry.rs");
  if (!fs.existsSync(cargoManifest) || !fs.existsSync(sourceEntry)) {
    return null;
  }

  const candidates = [
    path.join(PACKAGE_ROOT, "target", "release", `octo${WINDOWS_EXE}`),
    path.join(PACKAGE_ROOT, "target", "debug", `octo${WINDOWS_EXE}`),
    path.join(PACKAGE_ROOT, "target", "release", `octocode${WINDOWS_EXE}`),
    path.join(PACKAGE_ROOT, "target", "debug", `octocode${WINDOWS_EXE}`),
  ];

  return candidates.find((candidate) => fs.existsSync(candidate)) || null;
}

function downloadToFile(url, destination) {
  return new Promise((resolve, reject) => {
    const request = https.get(url, {
      headers: { "User-Agent": "octo-cli-bootstrap" },
    }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        return resolve(downloadToFile(res.headers.location, destination));
      }

      if (res.statusCode !== 200) {
        res.resume();
        reject(new Error(`Download failed with status ${res.statusCode} for ${url}`));
        return;
      }

      const out = fs.createWriteStream(destination);
      res.pipe(out);
      out.on("finish", () => {
        out.close(() => resolve(destination));
      });
      out.on("error", reject);
    });
    request.on("error", reject);
  });
}

function downloadText(url) {
  return new Promise((resolve, reject) => {
    const request = https.get(url, {
      headers: { "User-Agent": "octo-cli-bootstrap" },
    }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        return resolve(downloadText(res.headers.location));
      }

      if (res.statusCode !== 200) {
        res.resume();
        reject(new Error(`Download failed with status ${res.statusCode} for ${url}`));
        return;
      }

      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => {
        body += chunk;
      });
      res.on("end", () => resolve(body));
    });
    request.on("error", reject);
  });
}

function parseSha256Manifest(text) {
  const entries = new Map();
  for (const rawLine of String(text || "").split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const match = line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (!match) continue;
    const hash = match[1].toLowerCase();
    const filename = path.basename(match[2].trim());
    entries.set(filename, hash);
  }
  return entries;
}

function parseSingleSha256(text) {
  const match = String(text || "").match(/([a-fA-F0-9]{64})/);
  return match ? match[1].toLowerCase() : null;
}

function sha256File(filePath) {
  const hash = crypto.createHash("sha256");
  const data = fs.readFileSync(filePath);
  hash.update(data);
  return hash.digest("hex");
}

async function resolveExpectedSha256(artifact, options) {
  if (options.expectedHash) {
    return options.expectedHash.toLowerCase();
  }
  if (options.manifestText) {
    const manifest = parseSha256Manifest(options.manifestText);
    const expected = manifest.get(path.basename(artifact));
    if (expected) return expected;
  }
  if (options.manifestUrl) {
    try {
      const manifestText = await downloadText(options.manifestUrl);
      const manifest = parseSha256Manifest(manifestText);
      const expected = manifest.get(path.basename(artifact));
      if (expected) return expected;
    } catch (_) {
      // Fall through to artifact-specific checksum.
    }
  }
  if (options.singleHashText) {
    const expected = parseSingleSha256(options.singleHashText);
    if (expected) return expected;
  }
  if (options.singleHashUrl) {
    const checksumText = await downloadText(options.singleHashUrl);
    const expected = parseSingleSha256(checksumText);
    if (expected) return expected;
  }
  throw new Error(`No SHA256 checksum found for ${artifact}`);
}

async function verifyArtifactChecksum(archivePath, artifact, options = {}) {
  const expected = await resolveExpectedSha256(artifact, options);
  const actual = sha256File(archivePath);
  if (actual !== expected) {
    throw new Error(`Checksum mismatch for ${artifact}: expected ${expected}, got ${actual}`);
  }
  return { expected, actual };
}

async function extractArchive(archivePath, targetDir) {
  const isZip = archivePath.toLowerCase().endsWith(".zip");
  if (isZip) {
    try {
      runCommand("tar", ["-xf", archivePath, "-C", targetDir]);
      return;
    } catch (err) {
      if (process.platform !== "win32") {
        throw err;
      }
      runCommand("powershell", [
        "-NoProfile",
        "-Command",
        `Expand-Archive -LiteralPath '${archivePath}' -DestinationPath '${targetDir}' -Force`,
      ]);
      return;
    }
  }

  runCommand("tar", ["-xzf", archivePath, "-C", targetDir]);
}

async function ensureCliBinary(options = {}) {
  const { required = false, version: overrideVersion, dryRun = false } = options;
  const version = overrideVersion || getInstallVersion();
  const target = getTargetTriple();
  const artifact = getReleaseArtifactName(version, target);
  const downloadUrl = `${REPO_ORIGIN}/releases/download/v${version}/${artifact}`;
  const manifestUrl = `${REPO_ORIGIN}/releases/download/v${version}/SHA256SUMS`;
  const singleHashUrl = `${downloadUrl}.sha256`;
  const localBinaryPath = overrideVersion || process.env.OCTO_VERSION
    ? null
    : getLocalSourceBinaryPath();

  if (localBinaryPath) {
    return {
      action: dryRun ? "would-use-local" : "local",
      path: localBinaryPath,
      artifact,
      downloadUrl,
      checksumUrl: manifestUrl,
      dryRun,
      version,
      target,
    };
  }

  const binaryPath = getCachedBinaryPath(version, target);

  if (fs.existsSync(binaryPath)) {
    return {
      action: dryRun ? "would-use-cached" : "cached",
      path: binaryPath,
      artifact,
      downloadUrl,
      checksumUrl: manifestUrl,
      dryRun,
      version,
      target,
    };
  }

  if (dryRun) {
    return {
      action: "would-download",
      path: binaryPath,
      artifact,
      downloadUrl,
      checksumUrl: manifestUrl,
      dryRun: true,
      version,
      target,
    };
  }

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "octo-bootstrap-"));
  const archivePath = path.join(tmpDir, artifact);

  try {
    await downloadToFile(downloadUrl, archivePath);
    await verifyArtifactChecksum(archivePath, artifact, { manifestUrl, singleHashUrl });
    await extractArchive(archivePath, tmpDir);

    const extractedBinary = path.join(tmpDir, `octo${WINDOWS_EXE}`);
    const extractedAlternative = path.join(tmpDir, `octocode${WINDOWS_EXE}`);
    if (!fs.existsSync(extractedBinary) && !fs.existsSync(extractedAlternative)) {
      throw new Error(`Downloaded archive does not contain expected binary file: octo${WINDOWS_EXE}`);
    }

    fs.mkdirSync(path.dirname(binaryPath), { recursive: true });
    fs.copyFileSync(
      fs.existsSync(extractedBinary) ? extractedBinary : extractedAlternative,
      binaryPath
    );
    if (process.platform !== "win32") {
      fs.chmodSync(binaryPath, 0o755);
    }

    return {
      action: "downloaded",
      path: binaryPath,
      artifact,
      downloadUrl,
      checksumUrl: manifestUrl,
      dryRun: false,
      version,
      target,
    };
  } catch (error) {
    if (!required) {
      return {
        action: "missing",
        path: null,
        artifact,
        downloadUrl,
        checksumUrl: manifestUrl,
        dryRun: false,
        version,
        target,
        error: error.message,
      };
    }
    const hint = [
      "Could not download the prebuilt binary from GitHub Releases.",
      `URL: ${downloadUrl}`,
      "Make sure release assets exist and network access is available.",
      "Fallback: install with cargo (https://github.com/FanyinLiu/octo).",
      `error: ${error.message}`,
    ].join("\n");
    throw new Error(hint);
  } finally {
    if (fs.existsSync(tmpDir)) {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  }
}

function parseCliArgs(argv) {
  const options = { version: null, postinstall: false, dryRun: false };
  for (const arg of argv) {
    if (arg === "--postinstall") {
      options.postinstall = true;
    } else if (arg === "--dry-run") {
      options.dryRun = true;
    } else if (arg.startsWith("--version=")) {
      options.version = arg.split("=", 2)[1];
    }
  }
  return options;
}

async function main(argv) {
  const options = parseCliArgs(argv);
  const result = await ensureCliBinary({
    required: false,
    version: options.version ? options.version.replace(/^v/, "") : undefined,
    dryRun: options.dryRun,
  });

  if (options.dryRun) {
    if (result.action === "would-use-local" || result.action === "would-use-cached") {
      console.log(`[octo] Dry run: would use ${result.action === "would-use-local" ? "local" : "cached"} CLI at ${result.path}`);
    } else {
      console.log(`[octo] Dry run: would download ${result.artifact}`);
      console.log(`[octo] URL: ${result.downloadUrl}`);
      console.log(`[octo] Checksum: ${result.checksumUrl} or ${result.downloadUrl}.sha256`);
    }
    return;
  }

  if (!result.path) {
    const msg = [
      "[octo] Skipped binary bootstrap. No binary was downloaded.",
      "Run `octo --help` later; it will try to download the matching release on demand."
    ].join("\n");
    console.log(msg);
    return;
  }

  if (process.platform !== "win32") {
    fs.chmodSync(result.path, 0o755);
  }
  const verb = result.action === "downloaded" ? "installed" : "available";
  console.log(`[octo] CLI ${verb} at ${result.path}`);
}

if (require.main === module) {
  main(process.argv.slice(2)).catch((error) => {
    if (process.env.npm_config_user_agent && process.env.npm_config_user_agent.includes("npm")) {
      console.warn(`[octo] Postinstall bootstrap warning: ${error.message}`);
      process.exit(0);
    }
    console.error(`[octo] ${error.message}`);
    process.exit(1);
  });
}

module.exports = {
  ensureCliBinary,
  getTargetTriple,
  getReleaseArtifactName,
  parseSha256Manifest,
  sha256File,
  verifyArtifactChecksum,
};
