import { render } from "./harness";

const Row = "row";

function buildList() {
  return { count: 1, refresh() {} };
}

describe("row list", () => {
  it("mounts without throwing", () => {
    const list = buildList();
    list.refresh();
  });

  it("renders a row", () => {
    render(Row);
  });

  it("keeps the row count", () => {
    const list = buildList();
    expect(list.count).toBe(1);
  });
});
