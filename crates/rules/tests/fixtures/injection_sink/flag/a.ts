import { exec, execSync } from "node:child_process";

export function runUserCode(source: string): unknown {
  return eval(source);
}

export function compile(body: string): Function {
  return new Function("input", "return " + body);
}

export function listFiles(dir: string): void {
  exec(`ls -la ${dir}`, () => {});
}

export function checkout(branch: string): string {
  return execSync("git checkout " + branch).toString();
}

export function findUser(db: any, id: string) {
  return db.query(`select * from t where id = ${id}`);
}

export function searchByName(knex: any, name: string) {
  return knex.raw("select * from users where name = '" + name + "'");
}

export function rawLookup(prisma: any, id: string) {
  const q = `select * from users where id = ${id}`;
  return prisma.$queryRawUnsafe(q);
}

export function schedule(): void {
  const code = "doWork(" + Date.now() + ")";
  setTimeout(code, 10);
}
