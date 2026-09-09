/** Pins: `locrin:allow` on the catch line suppresses an empty catch. */
export function allowed(text: string): void {
  try {
    JSON.parse(text);
  } catch (err) {} // locrin:allow
}

/** Pins: a comment is not handling, so a comment-only catch is still empty. */
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
