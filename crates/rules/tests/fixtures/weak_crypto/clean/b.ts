import { createCipheriv, createHash, randomBytes, randomUUID } from "node:crypto";
import bcrypt from "bcrypt";

export function hashFile(buf: Buffer): string {
  return createHash("sha256").update(buf).digest("hex");
}

export async function storePassword(password: string): Promise<string> {
  return bcrypt.hash(password, 12);
}

export function jitter(): number {
  return Math.random() * 40;
}

export function shuffle<T>(items: T[]): T[] {
  return [...items].sort(() => Math.random() - 0.5);
}

export function encryptMessage(key: Buffer, plain: string): Buffer {
  const iv = randomBytes(16);
  const cipher = createCipheriv("aes-256-gcm", key, iv);
  return Buffer.concat([cipher.update(plain, "utf8"), cipher.final()]);
}

export function newRequestId(): string {
  return randomUUID();
}
