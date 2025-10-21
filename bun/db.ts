import { Database } from "bun:sqlite";
const db = new Database("./db.sqlite");
db.prepare(
  "create table if not exists checks (id INTEGER PRIMARY KEY AUTOINCREMENT, status INTEGER, time TEXT);",
).run();
db.prepare(
  "create table if not exists sing_record (id INTEGER PRIMARY KEY AUTOINCREMENT, type INTEGER, time TEXT);",
).run();
export default db;
