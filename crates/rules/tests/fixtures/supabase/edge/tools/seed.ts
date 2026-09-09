// A hand-run seeding script. It never ships, but tools/ is not one of the
// default server_paths, so the engine has to be told about it.
import { createClient } from "@supabase/supabase-js";

const admin = createClient(process.env.SUPABASE_URL!, process.env.SUPABASE_SERVICE_ROLE_KEY!);

export async function seed() {
  await admin.from("profiles").insert({ id: "00000000-0000-0000-0000-000000000000" });
}
