export function a(): number {
  return 1;
  // a trailing comment is not code
}
export function b(): number {
  return 2;
  var hoisted;
  var assigned = 3;
}
export function c(): number {
  return 3;
  console.log("allowed"); // locrin:allow
}
