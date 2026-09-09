import { createCipheriv, createDecipheriv, createHash } from "node:crypto";

export function hashPassword(password: string): string {
  return createHash("md5").update(password).digest("hex");
}

export function fingerprintToken(token: string): string {
  return crypto.createHash("SHA1").update(token).digest("hex");
}

export function newSessionToken(): string {
  return Math.random().toString(36).slice(2);
}

export function nextNonce(): string {
  const nonce = Math.random().toString(16).slice(2);
  return nonce;
}

export function encrypt(key: Buffer, plain: string): Buffer {
  const cipher = createCipheriv("aes-256-cbc", key, "0123456789abcdef");
  return Buffer.concat([cipher.update(plain, "utf8"), cipher.final()]);
}

export function decrypt(key: Buffer, blob: Buffer): Buffer {
  const decipher = createDecipheriv("aes-256-cbc", key, Buffer.alloc(16));
  return Buffer.concat([decipher.update(blob), decipher.final()]);
}
