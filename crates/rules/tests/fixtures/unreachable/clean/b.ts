export function a(x: number): number {
  if (x > 0) {
    return x;
  }
  return -x;
}
export function b(): number {
  return helper();
  function helper(): number {
    return 1;
  }
}
export function c(): void {
  throw new Error("x");
  type Local = number;
}
export function d(k: number): number {
  switch (k) {
    case 1:
      return 1;
    case 2: {
      return 2;
    }
    default:
      return 0;
  }
}
export const e = (): void => {
  return;
};
