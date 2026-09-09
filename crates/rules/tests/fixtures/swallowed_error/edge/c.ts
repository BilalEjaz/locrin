/** Pins: `locrin:allow` on the catch line suppresses an empty catch. */
export function allowed(text: string): void {
  try {
    JSON.parse(text);
  } catch (err) {} // locrin:allow
}

/** Pins: a comment-only catch is a deliberate ignore, not a swallow. */
export function commented(text: string): void {
  try {
    JSON.parse(text);
  } catch (err) {
    // ignore: best effort
  }
}

/** Pins: a try with only a finally has no catch, so nothing is swallowed. */
export function cleanup(text: string): void {
  try {
    JSON.parse(text);
  } finally {
    done();
  }
}

function done(): void {}

/** Pins: an async arrow bound to a const is a same-file async function. */
export const ping = async (): Promise<void> => {};

export function boot(): void {
  ping();
}

/** Pins: `this.<method>()` resolves to a same-file async method. */
export class Poller {
  async tick(): Promise<void> {}
  start(): void {
    this.tick();
  }
}

/** Pins: a comment-only catch in a function whose callers use its result is not
 * a log-only catch either; there is nothing in the body at all. */
export function widthOf(text: string): number {
  try {
    return JSON.parse(text).width;
  } catch (err) {
    // best effort: an unparsable payload has no width
  }
  return 0;
}

export function boxWidth(): number {
  return widthOf("{}") + 1;
}
