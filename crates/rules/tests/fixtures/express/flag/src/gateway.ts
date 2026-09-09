import express from "express";
import cors from "cors";
import { requireAuth } from "./auth";
import { search } from "./handlers";

const api = express();

api.use(cors({ origin: "*", credentials: true }));
api.get("/search", requireAuth, search);

export default api;
