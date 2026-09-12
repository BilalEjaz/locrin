#!/usr/bin/env node
"use strict";
// Stamps the version into every package.json under npm/ and copies binaries into
// the platform packages. Usage:
//   node npm/scripts/stage.js --version 0.5.0 --binary <path> --platform linux-x64 [--binary ... --platform ...]
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const PLATFORMS = ["linux-x64", "darwin-x64", "darwin-arm64", "win32-x64"];

function parse(argv) {
  const out = { version: null, pairs: [] };
  let pendingBinary = null;
  for (let i = 0; i < argv.length; i += 2) {
    const [flag, value] = [argv[i], argv[i + 1]];
    if (value === undefined) throw new Error(`missing value for ${flag}`);
    if (flag === "--version") out.version = value;
    else if (flag === "--binary") pendingBinary = value;
    else if (flag === "--platform") {
      if (!PLATFORMS.includes(value)) throw new Error(`unknown platform ${value}; expected one of ${PLATFORMS.join(", ")}`);
      if (!pendingBinary) throw new Error("--platform must follow --binary");
      out.pairs.push({ binary: pendingBinary, platform: value });
      pendingBinary = null;
    } else throw new Error(`unknown flag ${flag}`);
  }
  if (!out.version) throw new Error("--version is required");
  return out;
}

function stampJson(file, version) {
  const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
  pkg.version = version;
  if (pkg.optionalDependencies) for (const k of Object.keys(pkg.optionalDependencies)) pkg.optionalDependencies[k] = version;
  fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
}

function main() {
  const { version, pairs } = parse(process.argv.slice(2));
  stampJson(path.join(ROOT, "locrin", "package.json"), version);
  for (const p of PLATFORMS) stampJson(path.join(ROOT, "platforms", p, "package.json"), version);
  for (const { binary, platform } of pairs) {
    const dir = path.join(ROOT, "platforms", platform, "bin");
    fs.mkdirSync(dir, { recursive: true });
    const dest = path.join(dir, platform.startsWith("win32") ? "locrin.exe" : "locrin");
    fs.copyFileSync(binary, dest);
    if (process.platform !== "win32") fs.chmodSync(dest, 0o755);
  }
  console.log(`staged ${version}: ${pairs.map((p) => p.platform).join(", ") || "no binaries"}`);
}

main();
