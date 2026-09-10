// An edge function. It runs on Supabase's servers, never in the bundle, so the
// service role key belongs here and nowhere else.
import { createClient } from "@supabase/supabase-js";

const url = Deno.env.get("SUPABASE_URL")!;
const key = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!;

export const admin = createClient(url, key);

export async function purge(table: string) {
  await admin.from(table).delete().neq("id", "");
}
