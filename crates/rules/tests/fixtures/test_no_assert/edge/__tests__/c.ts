function expectPositive(value: number) {
  expect(value).toBeGreaterThan(0);
}

function checkTotal(total: number) {
  expectPositive(total);
}

describe("totals", () => {
  it("asserts through one helper", () => {
    expectPositive(2);
  });

  // Two levels of indirection is past what the extractor follows, so this case
  // reads as assertion-free and is reported. Pinned here on purpose.
  it("asserts through two helpers", () => {
    checkTotal(2);
  });

  test("takes an options object before its body", { timeout: 5000 }, () => {
    expectPositive(3);
  });

  // `it.skip.each` is not recognised as a case at all, so nothing is reported
  // for it either way. Pinned here on purpose.
  it.skip.each([[1], [2]])("is not seen as a case %i", (n: number) => {
    void n;
  });
});
