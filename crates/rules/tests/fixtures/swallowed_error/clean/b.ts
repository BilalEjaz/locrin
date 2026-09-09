import { readFile } from "node:fs/promises";

/** Pins: a catch that rethrows is handling the error, not swallowing it. */
export function strict(text: string): number {
  try {
    return JSON.parse(text).count;
  } catch (err) {
    throw new Error(`bad json: ${String(err)}`);
  }
}

/** Pins: a catch that logs and returns a failure value tells its callers. */
export function lenient(text: string): number | null {
  try {
    return JSON.parse(text).count;
  } catch (err) {
    console.error("bad json", err);
    return null;
  }
}

/** Pins: a log-only catch is form 2 only when a caller uses the result. */
export function report(text: string): void {
  try {
    JSON.parse(text);
  } catch (err) {
    console.error("bad json", err);
  }
}

/** Pins: calling a log-only function as a statement uses no result. */
export function run(): void {
  report("{}");
}

export async function refresh(): Promise<void> {
  await readFile("x", "utf8");
}

function done(): void {}

/** Pins: the four ways of not leaving a promise floating. */
export async function drive(): Promise<void> {
  await refresh();
  void refresh();
  refresh().catch(() => {});
  refresh().then(done);
}

/** Pins: a synchronous same-file call and an unknown import are not promises. */
export function plain(): void {
  done();
  readFile("y", "utf8");
}

/** Pins: a method sharing a name with a same-file async function is not it. */
export function detach(view: { refresh(): void }): void {
  view.refresh();
}

/** Pins: the exemption for `lenient` is the returned failure value, not the
 * absence of a caller: this one uses its result. */
export function count(): number {
  const n = lenient("{}");
  return n ?? 0;
}

/** Pins: a method's log-only catch is judged by `this.<name>()` callers only. A
 * free function of the same name, whose result a caller does use, says nothing
 * about the method. */
export class Loader {
  read(text: string): number {
    try {
      return JSON.parse(text).count;
    } catch (err) {
      console.error("bad json", err);
    }
    return 0;
  }
}

function read(text: string): number {
  return JSON.parse(text).count;
}

export function sum(text: string): number {
  return read(text) + 1;
}
