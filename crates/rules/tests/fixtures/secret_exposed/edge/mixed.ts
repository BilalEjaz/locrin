// The four decisions the rule makes that nothing in a pattern can make for it.

// 1. A service role key bypasses row level security, so it is a credential
// wherever it is written. A finding.
export const serviceRole = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";

// 2. The anon key beside it has the same shape and is meant to be in the client
// bundle. Not a finding, however credential-shaped the name is.
export const SUPABASE_ANON_KEY = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYW5vbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";

// 3. A line the repository has already decided about. Not a finding.
export const rotatedStripeKey = "sk_live_A1b2C3d4E5f6G7h8I9j0K1l2M3n4"; // locrin:allow

/*
 * 4. A key inside a block comment was pushed just the same, and git still has
 * it. A finding.
 * const token = "ghp_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8";
 */
export const ready = true;
