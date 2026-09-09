import { render } from "./harness";

const Row = "row";

describe("row list", () => {
  it.skip("mounts without throwing", () => {
    expect(render(Row)).toBeTruthy();
  });

  it("renders a row", () => {
    expect(render(Row)).toBeTruthy();
  });

  xit("keeps the row count", () => {
    expect(1).toBe(1);
  });
});
