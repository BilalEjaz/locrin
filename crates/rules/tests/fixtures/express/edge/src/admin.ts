import express from "express";
import { requireAuth } from "./auth";
import { listJobs, listUsers } from "./handlers";

const app = express();

app.use("/admin", requireAuth);

app.get("/admin/users", listUsers);
app.get("/jobs", listJobs);

export default app;
