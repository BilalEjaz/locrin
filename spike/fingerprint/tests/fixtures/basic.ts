export function add(a: number, b: number): number {
  const total = a + b;
  console.log("adding", total);
  return total;
}

export const multiply = (a: number, b: number): number => {
  const product = a * b;
  console.log("multiplying", product);
  return product;
};

class Calc {
  divide(a: number, b: number): number {
    if (b === 0) {
      throw new Error("div by zero");
    }
    return a / b;
  }
}

const tiny = () => 1;
