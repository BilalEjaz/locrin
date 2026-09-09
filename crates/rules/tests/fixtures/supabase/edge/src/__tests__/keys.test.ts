// A test needs the key that bypasses row level security in order to arrange the
// rows it then reads back through the anon key. Exempt by the default
// server_paths, which lists **/*.test.*.
import { createClient } from "@supabase/supabase-js";

const admin = createClient(process.env.SUPABASE_URL!, process.env.SUPABASE_SERVICE_ROLE_KEY!);

export async function seedRow(id: string) {
  await admin.from("profiles").insert({ id });
}
