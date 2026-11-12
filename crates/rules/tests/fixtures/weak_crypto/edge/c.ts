import { createHash } from "node:crypto";

export function etagFor(body: string): string {
  return createHash("md5").update(body).digest("hex");
}

export function render(): string {
  const sessionId = Math.random().toString(36).slice(2);
  return `<div data-key="${sessionId}" />`;
}

export class ResponseCache {
  readonly etag = createHash("md5").update("body").digest("hex");
  private readonly password = "unused";
}

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

export function acceptKey(key: string): string {
  return createHash("sha1").update(key + GUID).digest("base64");
}
