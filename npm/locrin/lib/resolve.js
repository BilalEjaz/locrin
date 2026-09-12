"use strict";
const PLATFORMS = {
  "linux-x64": "@raxbi/locrin-linux-x64",
  "darwin-x64": "@raxbi/locrin-darwin-x64",
  "darwin-arm64": "@raxbi/locrin-darwin-arm64",
  "win32-x64": "@raxbi/locrin-win32-x64",
};
const INSTALL = "https://github.com/BilalEjaz/locrin#install";

function packageFor(platform, arch) {
  const key = `${platform}-${arch}`;
  const name = PLATFORMS[key];
  if (!name) {
    const err = new Error(`locrin has no prebuilt binary for ${key}. See ${INSTALL} for the installer and the release page.`);
    err.code = "LOCRIN_UNSUPPORTED";
    throw err;
  }
  return name;
}

function binaryPath(platform = process.platform, arch = process.arch, resolve = require.resolve) {
  const name = packageFor(platform, arch);
  const file = platform === "win32" ? "locrin.exe" : "locrin";
  try {
    return resolve(`${name}/bin/${file}`);
  } catch (e) {
    const err = new Error(`locrin: the platform package ${name} is not installed. Optional dependencies may be disabled, or the lockfile predates this platform. Reinstall with optional dependencies enabled, or see ${INSTALL}.`);
    err.code = "LOCRIN_MISSING_PACKAGE";
    throw err;
  }
}

module.exports = { PLATFORMS, packageFor, binaryPath };
