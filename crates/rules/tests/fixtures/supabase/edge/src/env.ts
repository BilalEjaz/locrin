// The shape fastlift-admin has: a Cloudflare Worker module whose secrets arrive
// on a binding the platform passes in. Nothing here ships to a browser, and the
// interface below is TypeScript saying exactly that.
export interface Env {
  SUPABASE_URL: string;
  SUPABASE_SERVICE_ROLE_KEY: string;
}

export function restHeaders(env: Env): Record<string, string> {
  return {
    apikey: env.SUPABASE_SERVICE_ROLE_KEY,
    Authorization: `Bearer ${env.SUPABASE_SERVICE_ROLE_KEY}`,
  };
}
