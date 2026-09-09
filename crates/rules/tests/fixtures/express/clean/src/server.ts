import express from "express";
import cors from "cors";
import { requireAuth } from "./auth";
import { health, listUsers, login } from "./handlers";

const app = express();

app.use(cors({ origin: ["https://a.example.com"], credentials: true }));
app.use(requireAuth);

app.get("/health", health);
app.post("/login", login);
app.get("/users", listUsers);

app.get("/session", (req, res) => {
  res.cookie("session", req.token, { httpOnly: true, secure: true, sameSite: "lax" });
  res.clearCookie("stale");
  res.json({ ok: true });
});

export default app;
