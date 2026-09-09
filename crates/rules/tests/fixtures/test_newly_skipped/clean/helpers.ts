// Not a test file by name, so nothing in it is a test case however much it
// looks like one: the rule reads the naming convention, not the call.
export const it = { skip: (name: string, body: () => void) => ({ name, body }) };

it.skip("looks like a skipped case but is not in a test file", () => {});
