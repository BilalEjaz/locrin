mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::Config;
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::weak_crypto::WeakCrypto;

const HASH_FIX: &str = "Use bcrypt, scrypt, or argon2 for credentials; SHA-256 or better for integrity";
const RANDOM_FIX: &str = "Use crypto.randomBytes, crypto.randomUUID, or crypto.getRandomValues";
const IV_FIX: &str =
    "Generate a fresh random IV per message with crypto.randomBytes and store it beside the ciphertext";

#[test]
fn every_form_flags_its_two_lines_in_the_fixture() {
    let out = run_on(Box::new(WeakCrypto), &fixture("weak_crypto", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![
            ("a.ts".to_string(), 4),
            ("a.ts".to_string(), 8),
            ("a.ts".to_string(), 12),
            ("a.ts".to_string(), 16),
            ("a.ts".to_string(), 21),
            ("a.ts".to_string(), 26),
            ("a.ts".to_string(), 33),
        ]
    );
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec![
            "md5 used to hash password",
            "SHA1 used to hash token",
            "Math.random() generates newSessionToken",
            "Math.random() generates nonce",
            "static IV passed to createCipheriv",
            "static IV passed to createDecipheriv",
            "static IV passed to createDecipheriv",
        ]
    );
    let fixes: Vec<&str> = out.iter().map(|f| f.fix.as_str()).collect();
    assert_eq!(fixes, vec![HASH_FIX, HASH_FIX, RANDOM_FIX, RANDOM_FIX, IV_FIX, IV_FIX, IV_FIX]);
}

/// Spec 7.1 metadata: every finding is a High-severity Security finding under
/// A02:2021, and the CWE says which form spoke. See the plan's global
/// constraints.
#[test]
fn every_finding_carries_the_security_metadata_for_its_form() {
    let out = run_on(Box::new(WeakCrypto), &fixture("weak_crypto", "flag"), &Config::default());
    assert!(
        out.iter().all(|f| f.severity == Severity::High
            && f.category == Category::Security
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A02:2021")),
        "{:?}",
        out.first()
    );
    let cwes: Vec<Option<&str>> = out.iter().map(|f| f.cwe.as_deref()).collect();
    assert_eq!(
        cwes,
        vec![
            Some("CWE-327"),
            Some("CWE-327"),
            Some("CWE-338"),
            Some("CWE-338"),
            Some("CWE-327"),
            Some("CWE-327"),
            Some("CWE-327"),
        ]
    );
}

/// Two findings in one file do not share an id: the anchor carries what the
/// finding says as well as the symbol it sits in.
#[test]
fn findings_have_distinct_ids() {
    let out = run_on(Box::new(WeakCrypto), &fixture("weak_crypto", "flag"), &Config::default());
    let mut ids: Vec<&str> = out.iter().map(|f| f.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), out.len(), "seven findings, seven ids");
}

#[test]
fn strong_algorithms_and_non_credential_randomness_are_left_alone() {
    let out = run_on(Box::new(WeakCrypto), &fixture("weak_crypto", "clean"), &Config::default());
    assert!(out.is_empty(), "{:?}", hits(&out));
}

/// The two judgement calls. `etagFor` hashes a response body with md5 and names
/// no credential, so the finding stands at Medium: the algorithm is weak either
/// way, but nothing here says a credential is at stake. `sessionId` is a
/// credential-shaped name even though the function around it is `render`, so
/// the assignment target carries the confidence, not the enclosing symbol. The
/// class field on line 13 is the third: the context a hash is judged by is the
/// field it initialises, not every name in the class around it, so the unused
/// `password` field below it does not raise the etag hash to High.
#[test]
fn confidence_follows_the_credential_context_not_the_enclosing_function() {
    let out = run_on(Box::new(WeakCrypto), &fixture("weak_crypto", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("c.ts".to_string(), 4), ("c.ts".to_string(), 8), ("c.ts".to_string(), 13)]);
    assert_eq!(out[0].confidence, Confidence::Medium, "{}", out[0].evidence);
    assert_eq!(out[0].evidence, "md5 used to hash etagFor");
    assert_eq!(out[1].confidence, Confidence::High, "{}", out[1].evidence);
    assert_eq!(out[1].evidence, "Math.random() generates sessionId");
    assert_eq!(out[2].confidence, Confidence::Medium, "{}", out[2].evidence);
    assert_eq!(out[2].evidence, "md5 used to hash ResponseCache");
}
