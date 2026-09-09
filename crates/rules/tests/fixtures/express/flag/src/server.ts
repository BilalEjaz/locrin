import express from "express";
import cors from "cors";
import { requireAuth } from "./auth";
import { createUser, health, listUsers, reports } from "./handlers";

const app = express();

app.get("/health", health);
app.get("/admin/users", listUsers);
app.post("/admin/users", createUser);

app.get("/reports", requireAuth, cors(), reports);

app.get("/profile", requireAuth, (req, res) => {
  res.cookie("session", req.token);
  res.json({ ok: true });
});

export default app;
