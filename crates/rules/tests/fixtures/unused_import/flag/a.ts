import { used, unused } from "./lib";
import type { OnlyType } from "./types";
import * as ns from "./ns";
import def from "./def";

export function run(): number {
  return used(1);
}
