const test = require("node:test");
const assert = require("node:assert/strict");
const { run } = require("../bin/locrin");

const node = () => process.execPath;
const quiet = { write() {} };

test("forwards exit codes 0, 1 and 2", () => {
  assert.equal(run(["-e", "process.exit(0)"], { binaryPath: node, stderr: quiet }), 0);
  assert.equal(run(["-e", "process.exit(1)"], { binaryPath: node, stderr: quiet }), 1);
  assert.equal(run(["-e", "process.exit(2)"], { binaryPath: node, stderr: quiet }), 2);
});

test("unsupported platform exits 2 with the message", () => {
  let msg = "";
  const err = new Error("locrin has no prebuilt binary for plan9-mips"); err.code = "LOCRIN_UNSUPPORTED";
  const code = run([], { binaryPath: () => { throw err; }, stderr: { write(s) { msg += s; } } });
  assert.equal(code, 2);
  assert.match(msg, /plan9-mips/);
});

test("a binary that cannot start exits 2", () => {
  let msg = "";
  const code = run([], { binaryPath: () => "/definitely/not/here/locrin", stderr: { write(s) { msg += s; } } });
  assert.equal(code, 2);
  assert.match(msg, /could not start/);
});
