export function a(x: number): number {
  return x;
  console.log("never");
}
export function b(): void {
  throw new Error("boom");
  cleanup();
  more();
}
export function c(xs: number[]): number {
  for (const x of xs) {
    if (x > 1) {
      continue;
      xs.push(x);
    }
    break;
    xs.pop();
  }
  return 0;
}
export function d(k: number): string {
  switch (k) {
    case 1:
      return "one";
      break;
    default:
      return "other";
  }
}
function cleanup() {}
function more() {}
