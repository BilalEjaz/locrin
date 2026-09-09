import { execFile, spawn } from "node:child_process";
import sql from "sql-template-tag";

export function constant(): unknown {
  return eval("2 + 2");
}

export function byId(id: string) {
  return sql`select * from users where id = ${id}`;
}

export function rawById(prisma: any, id: string) {
  return prisma.$queryRaw`select * from users where id = ${id}`;
}

export function one(db: any) {
  return db.query("select 1");
}

export function byIdParameterised(db: any, id: string) {
  return db.query("select * from t where id = $1", [id]);
}

export function list(dir: string): void {
  execFile("ls", [dir], () => {});
}

export function status(): void {
  spawn("git", ["status"]);
}

export function later(): void {
  setTimeout(() => {}, 10);
}
