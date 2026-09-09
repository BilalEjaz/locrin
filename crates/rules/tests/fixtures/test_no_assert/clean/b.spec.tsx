import { render } from "./harness";

const Row = () => <div />;

function expectRowShape(row: unknown) {
  expect(row).toBeDefined();
}

describe("row", () => {
  it("delegates its check to a helper", () => {
    expectRowShape({ id: 1 });
  });

  it("uses a node assert", () => {
    assert.deepEqual({ id: 1 }, { id: 1 });
  });

  it("uses a chai should chain", () => {
    const row = { id: 1 };
    row.should.have.property("id");
  });

  it("declares how many assertions it makes", () => {
    expect.assertions(1);
    expect(render(<Row />)).toBeTruthy();
  });

  it.skip("is skipped and asserts nothing", () => {
    render(<Row />);
  });

  it.todo("covers the empty state");

  it("checks the row with a throwing query", () => {
    const { getByText } = render(<Row />);
    getByText("row");
  });
});
