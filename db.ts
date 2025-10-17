import { Database } from "bun:sqlite";
const db = new Database(":memory:");
db.prepare("create table checks (id INTEGER PRIMARY KEY AUTOINCREMENT, status INTEGER, time TEXT);").run();
db.prepare(`insert into checks (status, time) values (1, '${new Date().toISOString()}');`).run();
export default db;