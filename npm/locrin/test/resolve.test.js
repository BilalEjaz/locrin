const test = require("node:test");
const assert = require("node:assert/strict");
const { packageFor, binaryPath, PLATFORMS } = require("../lib/resolve");

test("maps every supported platform to its package", () => {
  assert.equal(packageFor("linux", "x64"), "@raxbi/locrin-linux-x64");
  assert.equal(packageFor("darwin", "x64"), "@raxbi/locrin-darwin-x64");
  assert.equal(packageFor("darwin", "arm64"), "@raxbi/locrin-darwin-arm64");
  assert.equal(packageFor("win32", "x64"), "@raxbi/locrin-win32-x64");
  assert.equal(Object.keys(PLATFORMS).length, 4);
});

test("names the platform when there is no prebuilt binary", () => {
  assert.throws(() => packageFor("freebsd", "x64"), (e) => e.code === "LOCRIN_UNSUPPORTED" && /freebsd-x64/.test(e.message) && /install/.test(e.message));
});

test("resolves the binary inside the platform package", () => {
  const seen = [];
  const p = binaryPath("linux", "x64", (spec) => { seen.push(spec); return "/abs/" + spec; });
  assert.equal(p, "/abs/@raxbi/locrin-linux-x64/bin/locrin");
  const w = binaryPath("win32", "x64", (spec) => "/abs/" + spec);
  assert.equal(w, "/abs/@raxbi/locrin-win32-x64/bin/locrin.exe");
  assert.deepEqual(seen, ["@raxbi/locrin-linux-x64/bin/locrin"]);
});

test("explains a missing platform package", () => {
  assert.throws(() => binaryPath("linux", "x64", () => { throw new Error("Cannot find module"); }),
    (e) => e.code === "LOCRIN_MISSING_PACKAGE" && /@raxbi\/locrin-linux-x64/.test(e.message) && /optional/i.test(e.message));
});
