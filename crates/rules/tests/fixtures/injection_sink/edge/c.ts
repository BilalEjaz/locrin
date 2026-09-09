import { exec } from "node:child_process";

export function listing(): void {
  exec("ls " + "-la", () => {});
}

export function all(db: any) {
  const q = "select * from users";
  return db.query(q);
}

export function unsafe(db: any, id: string) {
  return db.query(`select * from t where id = ${id}`); // locrin:allow
}
