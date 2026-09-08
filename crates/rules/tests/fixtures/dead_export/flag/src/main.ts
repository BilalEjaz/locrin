import { used } from "./lib";
import * as all from "./ns";
import { y } from "./barrel";
export const run = (): number => used() + all.x + y;
