// An unclosed JSX element: tree-sitter recovers by swallowing the imports below it,
// so the only edge into src/b.ts exists in the text and not in the tree.
const shell = <View>;

import { useB, B_LIMIT } from "./b";

export function Main() {
  return useB() + B_LIMIT;
}
