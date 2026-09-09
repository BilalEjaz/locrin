// Not a test file by name, so the rule never looks at it, even though it holds
// something shaped exactly like an assertion-free case.
export function register(it: (name: string, body: () => void) => void) {
  it("looks like a case but is not in a test file", () => {
    register(it);
  });
}
