import { readFile } from "node:fs/promises";

export async function loadConfig(path: string): Promise<string> {
  try {
    return await readFile(path, "utf8");
  } catch (e) {}
  return "";
}

export function parseCount(text: string): number {
  try {
    return JSON.parse(text).count;
  } catch (err) {
    console.error("bad json", err);
  }
}

export function total(): number {
  const n = parseCount("{}");
  return n + 1;
}

export async function refresh(): Promise<void> {
  await loadConfig("x");
}

export function boot(): void {
  refresh();
  loadConfig("y");
}
