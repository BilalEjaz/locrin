//! Reads the role out of a JSON Web Token, which is what tells a Supabase
//! service role key from the anon key sitting next to it in the same file.
//!
//! Both are JWTs, both are long, both are signed by the same project, and one
//! of them is meant to be in the client bundle. Nothing in the shape separates
//! them: only the payload does. Decoding it is safe to do here because the
//! payload of a JWT is not encrypted, only signed; this reads it, never trusts
//! it for anything but a label, and never verifies a signature.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// The `role` claim of a token's payload, or `None` when there is no second
/// segment, the segment is not base64url, the payload is not JSON, or it has no
/// `role`.
///
/// Supabase writes `service_role` for the key that bypasses row level security
/// and `anon` or `authenticated` for the two that do not.
pub fn role(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(json.get("role")?.as_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three fixture tokens, synthetic and unsigned by anything real.
    const SERVICE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
    const ANON: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYW5vbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";

    #[test]
    fn reads_the_role_claim_and_says_nothing_when_there_is_none() {
        assert_eq!(role(SERVICE).as_deref(), Some("service_role"));
        assert_eq!(role(ANON).as_deref(), Some("anon"));
        assert_eq!(role("not.a.token"), None, "the payload is not base64url JSON");
        assert_eq!(role("eyJhbGciOiJIUzI1NiJ9"), None, "one segment, no payload");
        // A well formed payload with no role: an ordinary application token.
        assert_eq!(role("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.sig"), None);
    }
}
