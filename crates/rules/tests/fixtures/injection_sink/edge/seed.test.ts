export function seed(db: any, name: string) {
  return db.query(`insert into users (name) values ('${name}')`);
}
