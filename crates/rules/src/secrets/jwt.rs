//! Reads the role out of a JSON Web Token, which is what tells a Supabase
//! service role key from the anon key sitting next to it in the same file.
//!
//! Both are JWTs, both are long, both are signed by the same project, and one
//! of them is meant to be in the client bundle. Nothing in the shape separates
//! them: only the payload does. Decoding it is safe to do here because the
//! payload of a JWT is not encrypted, only signed; this reads it, never trusts
//! it for anything but a label, and never verifies a signature.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// The payload of a token as JSON, or `None` when there is no second segment,
/// the segment is not base64url, or it is not an object.
fn claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The `role` claim of a token's payload, or `None` when there is no payload to
/// read or it has no `role`.
///
/// Supabase writes `service_role` for the key that bypasses row level security
/// and `anon` or `authenticated` for the two that do not.
pub fn role(token: &str) -> Option<String> {
    Some(claims(token)?.get("role")?.as_str()?.to_string())
}

/// Whether the token's `exp` claim is behind `now`, in unix seconds.
///
/// An expired token is not a credential: there is nothing to revoke and nothing
/// for a locked rule to block a build over, and a repository that keeps an old
/// key around in a fixture or a migration note should not have to argue with
/// the engine about it. Only a claim that says so counts: a token with no `exp`,
/// an `exp` that is not a number, or a payload this cannot read is treated as
/// live, because the rule must never discard a credential it failed to parse.
pub fn expired_at(token: &str, now: i64) -> bool {
    claims(token).and_then(|c| c.get("exp")?.as_i64()).is_some_and(|exp| exp < now)
}

/// [`expired_at`] against the system clock. A clock before the epoch reads as
/// zero, which makes every token live rather than every token expired.
pub fn expired(token: &str) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    expired_at(token, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture tokens, synthetic and unsigned by anything real.
    const SERVICE: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
    const ANON: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoiYW5vbiIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoyMDAwMDAwMDAwfQ.SyntheticSignatureForLocrinFixturesOnly0000000";
    /// The same service role payload with `iat` 1600000000 and `exp` an hour
    /// later, both in the past.
    const EXPIRED: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNjAwMDAwMDAwLCJleHAiOjE2MDAwMDM2MDB9.SyntheticSignatureForLocrinFixturesOnly0000000";

    #[test]
    fn reads_the_role_claim_and_says_nothing_when_there_is_none() {
        assert_eq!(role(SERVICE).as_deref(), Some("service_role"));
        assert_eq!(role(ANON).as_deref(), Some("anon"));
        assert_eq!(role("not.a.token"), None, "the payload is not base64url JSON");
        assert_eq!(role("eyJhbGciOiJIUzI1NiJ9"), None, "one segment, no payload");
        // A well formed payload with no role: an ordinary application token.
        assert_eq!(role("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.sig"), None);
    }

    /// A token whose `exp` has passed is not a live credential. `now` is a
    /// parameter so the test pins a moment rather than asking the clock: the
    /// expired token below ran out at 1600003600, and the live one at
    /// 2000000000, and both answers have to hold in 2035 as well as today.
    #[test]
    fn a_token_whose_expiry_has_passed_is_not_live() {
        const AFTER: i64 = 1_700_000_000;
        assert!(expired_at(EXPIRED, AFTER), "exp 1600003600 is behind 1700000000");
        assert!(!expired_at(SERVICE, AFTER), "exp 2000000000 is ahead of it");
        assert!(!expired_at(EXPIRED, 1_600_000_001), "a second into its hour it was still live");
        // No `exp`, no payload, no token: nothing says it has expired, so it has
        // not. A rule that cannot read a claim must not discard a credential.
        assert!(!expired_at("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.sig", AFTER));
        assert!(!expired_at("not.a.token", AFTER));
        assert!(!expired_at("", AFTER));
    }
}
