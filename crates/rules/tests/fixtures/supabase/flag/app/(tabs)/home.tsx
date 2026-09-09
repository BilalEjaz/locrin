// A screen in the client bundle. Everything below ships to the device.
import { createClient } from "@supabase/supabase-js";

const url = process.env.EXPO_PUBLIC_SUPABASE_URL!;
const key = process.env.SUPABASE_SERVICE_ROLE_KEY!;

export const admin = createClient(url, key);

// A synthetic token, signed by nothing, with a service_role payload.
const fallback =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";

export async function purge(table: string) {
  await admin.from(table).delete().neq("id", "");
  return fallback;
}
