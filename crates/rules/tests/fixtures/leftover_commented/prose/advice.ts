export function decide(): number {
  // OQ8: four callers wrote their own recovery advice into the failure path,
  // which duplicates the suggestion table.
  // let the suggestion table carry the advice (it already holds the text).
  // The callers are keyboard.ts, pointer.ts and the two focus helpers.
  return 1;
}
