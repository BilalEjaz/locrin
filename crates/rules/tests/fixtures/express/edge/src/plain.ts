import { createServer } from "node:http";

const app = createServer();

app.get("/admin/users", () => {});

export function setCookie(res) {
  res.cookie("session", "abc");
}
