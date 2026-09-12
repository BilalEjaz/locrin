#!/usr/bin/env node
"use strict";
const { spawnSync } = require("node:child_process");
const { binaryPath } = require("../lib/resolve");

// Runs the platform binary with the same arguments and returns its exit code.
// Exit codes are the contract: 0 pass or advisory, 1 block, 2 engine error.
function run(argv, opts = {}) {
  const stderr = opts.stderr || process.stderr;
  const resolveBinary = opts.binaryPath || binaryPath;
  let bin;
  try {
    bin = resolveBinary();
  } catch (e) {
    stderr.write(`${e.message}\n`);
    return 2;
  }
  const r = spawnSync(bin, argv, { stdio: "inherit", windowsHide: true });
  if (r.error) {
    stderr.write(`locrin: could not start ${bin}: ${r.error.message}\n`);
    return 2;
  }
  return r.status === null ? 2 : r.status;
}

module.exports = { run };
if (require.main === module) process.exit(run(process.argv.slice(2)));
