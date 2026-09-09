import { createHash } from "node:crypto";

export function etagFor(body: string): string {
  return createHash("md5").update(body).digest("hex");
}

export function render(): string {
  const sessionId = Math.random().toString(36).slice(2);
  return `<div data-key="${sessionId}" />`;
}
