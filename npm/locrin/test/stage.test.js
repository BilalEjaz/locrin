const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

test("stage stamps versions and copies binaries", () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "locrin-stage-"));
  fs.cpSync(path.join(__dirname, "..", ".."), root, { recursive: true, filter: (p) => !p.includes("node_modules") });
  const bin = path.join(root, "fake-locrin"); fs.writeFileSync(bin, "#!/bin/sh\necho locrin 9.9.9\n");
  const exe = path.join(root, "fake-locrin.exe"); fs.writeFileSync(exe, "MZ");
  execFileSync(process.execPath, [path.join(root, "scripts", "stage.js"), "--version", "9.9.9",
    "--binary", bin, "--platform", "linux-x64", "--binary", exe, "--platform", "win32-x64"], { cwd: root });
  const entry = JSON.parse(fs.readFileSync(path.join(root, "locrin", "package.json"), "utf8"));
  assert.equal(entry.version, "9.9.9");
  for (const v of Object.values(entry.optionalDependencies)) assert.equal(v, "9.9.9");
  const linux = JSON.parse(fs.readFileSync(path.join(root, "platforms", "linux-x64", "package.json"), "utf8"));
  assert.equal(linux.version, "9.9.9");
  assert.ok(fs.existsSync(path.join(root, "platforms", "linux-x64", "bin", "locrin")));
  assert.ok(fs.existsSync(path.join(root, "platforms", "win32-x64", "bin", "locrin.exe")));
  if (process.platform !== "win32") assert.ok(fs.statSync(path.join(root, "platforms", "linux-x64", "bin", "locrin")).mode & 0o111);
});

test("stage refuses an unknown platform", () => {
  assert.throws(() => execFileSync(process.execPath, [path.join(__dirname, "..", "..", "scripts", "stage.js"),
    "--version", "1.0.0", "--binary", __filename, "--platform", "beos-ppc"], { stdio: "pipe" }));
});
