mod common;

use std::collections::BTreeSet;

use common::{fixture, hits, run_on};
use locrin_core::config::{Config, RuleOverride};
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity};
use locrin_rules::secrets::patterns::PATTERNS;
use locrin_rules::secrets::SecretExposed;

/// The provider named in a finding's evidence, which is everything before
/// ` credential: `.
fn provider(evidence: &str) -> &str {
    evidence.split(" credential: ").next().unwrap_or(evidence)
}

/// Every string literal in the flag fixture long enough to be a credential.
/// The masking test needs the values themselves to prove none of them leaked.
fn fixture_values() -> Vec<String> {
    let path = fixture("secret_exposed", "flag").join("keys.ts");
    let source = std::fs::read_to_string(path).unwrap();
    source.split('"').skip(1).step_by(2).filter(|s| s.len() >= 20).map(|s| s.to_string()).collect()
}

#[test]
fn every_pattern_in_the_table_flags_its_line_in_the_fixture() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let found: BTreeSet<&str> = out.iter().map(|f| provider(&f.evidence)).collect();
    let missing: Vec<&str> = PATTERNS.iter().map(|p| p.provider).filter(|p| !found.contains(p)).collect();
    assert!(missing.is_empty(), "no fixture line for: {missing:?}");
    assert!(PATTERNS.len() >= 100, "the table is the rule: {} patterns", PATTERNS.len());
    // `keys.ts` is one line per entry in the table and nothing else, so the
    // count of findings in it is the count of entries exactly. One more means an
    // entry fired on another entry's line, which is the failure mode a set of
    // provider names cannot see: a new pattern that also matches the line above
    // it looks like coverage and is a second finding on somebody's build.
    let on_keys: Vec<&str> =
        out.iter().filter(|f| f.file == "keys.ts").map(|f| provider(&f.evidence)).collect::<Vec<_>>();
    let mut crossed: Vec<&&str> = on_keys.iter().filter(|p| on_keys.iter().filter(|q| q == p).count() > 1).collect();
    crossed.dedup();
    assert_eq!(on_keys.len(), PATTERNS.len(), "an entry fired twice or on another entry's line: {crossed:?}");
    // Every finding carries the security metadata spec 7.1 asks for, at the
    // severity and confidence the plan fixes for this rule.
    assert!(
        out.iter().all(|f| f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.category == Category::Security
            && f.owasp.as_deref() == Some("A02:2021")
            && f.cwe.as_deref() == Some("CWE-798")),
        "{:?}",
        out.first()
    );
    assert!(out.iter().all(|f| f.fix == "Revoke the credential now, move it to an environment variable or secret store, and purge it from git history"), "{:?}", out[0].fix);
}

#[test]
fn evidence_carries_the_provider_and_a_mask_and_never_the_value() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let values = fixture_values();
    assert!(values.len() > 100, "the fixture holds one value per pattern: {}", values.len());
    for finding in &out {
        let evidence = &finding.evidence;
        let (_, mask) = evidence.split_once(" credential: ").expect("{provider} credential: {mask}");
        // Split on the ellipsis from the right: a head of four characters can
        // itself end in a dot (`hvs.`), so the first `...` in the string is not
        // always the separator.
        let (head, tail) = mask.rsplit_once("...(").expect("the mask is head...(len chars)");
        assert!(head.chars().count() <= 4, "{evidence}");
        assert!(tail.ends_with(" chars)"), "{evidence}");
        // Four characters of a value may appear in the mask, and no more. Five
        // characters of any fixture value would be a leak. Two things are
        // exempt: the provider half, which is prose (a provider named "AWS
        // access key ID" is not a value), and a value whose fifth character is
        // the `.` that the ellipsis starts with, where the five character run
        // is an accident of the separator rather than a character of the value.
        for value in values.iter().filter(|v| v.as_bytes()[4] != b'.') {
            assert!(!mask.contains(&value[..5]), "{evidence} leaks {}", &value[..5]);
        }
    }
}

#[test]
fn the_finding_is_anchored_on_the_provider_and_a_hash_of_the_value_not_the_value() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "edge"), &Config::default());
    let value = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6ImFiY2RlZmdoaWprbG1ub3AiLCJyb2xlIjoic2VydmljZV9yb2xlIiwiaWF0IjoxNzAwMDAwMDAwLCJleHAiOjIwMDAwMDAwMDB9.SyntheticSignatureForLocrinFixturesOnly0000000";
    let anchor = format!("Supabase service role key\x1f{}", &blake3::hash(value.as_bytes()).to_hex()[..8]);
    assert_eq!(out[0].id, make_id("secret-exposed", "mixed.ts", &anchor));
}

/// The three structural placeholder tokens (`<...>`, `${`, `process.env`) say
/// something about the value, not about the line. Read line-wide they hid a key
/// in a tag's props, a key inside a `useState<string>(...)`, a key in a template
/// literal that interpolates something else, and the fallback beside an
/// environment reference, which is the value that actually ships.
#[test]
fn a_structural_token_beside_a_key_does_not_excuse_the_key() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let mut seen: Vec<(u32, &str)> =
        out.iter().filter(|f| f.file == "inline.tsx").map(|f| (f.span.start_line, provider(&f.evidence))).collect();
    seen.sort();
    assert_eq!(
        seen,
        // One line, one key, one finding: the named provider claims the value
        // and the generic `apiKey =` entry steps aside on line 7.
        vec![(7, "Google API key"), (11, "Stripe secret key"), (16, "Google API key"), (20, "Google API key")]
    );
}

/// A JSON style key is still a credential name.
///
/// The delimiter that closed the identifier hole asked for the separator
/// immediately after the name, so every quoted key stopped matching: an
/// `Authorization` header inside a headers object, an AWS profile written as
/// JSON, a config map. The closing quote in front of the separator is optional
/// and the opening quote after it is not, so
/// `"expoToken": getExpoTokenFromSecureStore()` in the clean fixture is still
/// a name beside an identifier and still clean.
#[test]
fn a_quoted_credential_name_is_still_a_credential_name() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let mut seen: Vec<(u32, &str)> = out
        .iter()
        .filter(|f| f.file == "decisions.ts")
        .filter(|f| matches!(provider(&f.evidence), "bearer token literal" | "AWS secret access key"))
        .map(|f| (f.span.start_line, f.evidence.as_str()))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            (25, "bearer token literal credential: Bd3Y...(24 chars)"),
            (26, "AWS secret access key credential: zQ8m...(40 chars)"),
        ]
    );
}

/// Private keys in one file are one decision each. Anchored on the header they
/// were one decision for all of them, because the header is the same string in
/// every repository; the anchor is the hash of the key material, so they are
/// three. The third is a legacy encrypted PEM, whose base64 body sits four
/// lines below the header behind the two headers OpenSSL writes for it, and
/// whose material is therefore the `Proc-Type:` line rather than base64.
#[test]
fn private_keys_in_one_file_are_separate_findings_with_separate_ids() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let keys: Vec<&Finding> =
        out.iter().filter(|f| f.file == "decisions.ts" && provider(&f.evidence) == "Private key block").collect();
    // The evidence masks the material, not the header every key block shares.
    let seen: Vec<(u32, &str)> = keys.iter().map(|f| (f.span.start_line, f.evidence.as_str())).collect();
    assert_eq!(
        seen,
        vec![
            (12, "Private key block credential: MIIE...(63 chars)"),
            (16, "Private key block credential: MIIE...(63 chars)"),
            (31, "Private key block credential: Proc...(22 chars)"),
        ],
        "{keys:?}"
    );
    let ids: BTreeSet<&str> = keys.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids.len(), keys.len(), "three keys, {} ids", ids.len());
}

/// The privileged role list is positive. `admin` and `superuser` are
/// credentials on their own; `viewer` is one only because the line calls it a
/// token, and the clean fixture holds the same token under a name that does not.
#[test]
fn a_privileged_role_is_a_credential_and_any_other_role_needs_a_credential_name() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "flag"), &Config::default());
    let mut jwts: Vec<(&str, u32, &str)> = out
        .iter()
        .filter(|f| provider(&f.evidence) == "JSON Web Token")
        .map(|f| (f.file.as_str(), f.span.start_line, f.evidence.as_str()))
        .collect();
    jwts.sort();
    assert_eq!(
        jwts,
        vec![
            // The role is privileged on its own.
            ("decisions.ts", 5, "JSON Web Token credential: eyJh...(176 chars)"),
            // The role is not, and the name on the line says token.
            ("decisions.ts", 8, "JSON Web Token credential: eyJh...(172 chars)"),
            // The role is privileged on its own.
            ("keys.ts", 93, "JSON Web Token credential: eyJh...(207 chars)"),
        ],
        "admin, superuser, and the viewer token on a credential name"
    );
}

#[test]
fn environment_references_placeholders_public_identifiers_and_dev_uris_are_clean() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

#[test]
fn a_service_role_jwt_and_a_key_in_a_block_comment_are_findings_an_anon_jwt_and_an_allowed_line_are_not() {
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "edge"), &Config::default());
    assert_eq!(hits(&out), vec![("mixed.ts".into(), 5), ("mixed.ts".into(), 17)], "{out:?}");
    assert_eq!(
        out.iter().map(|f| provider(&f.evidence)).collect::<Vec<_>>(),
        vec!["Supabase service role key", "GitHub token"]
    );
}

/// Spec 4.3: the rule is locked. A config that disables it and lowers its
/// severity changes neither.
#[test]
fn the_config_can_neither_disable_the_rule_nor_lower_its_severity() {
    let mut rules = std::collections::BTreeMap::new();
    rules.insert(
        "secret-exposed".to_string(),
        RuleOverride { enabled: Some(false), severity: Some(Severity::Low), languages: None },
    );
    let config = Config { rules, ..Config::default() };
    let out = run_on(Box::new(SecretExposed), &fixture("secret_exposed", "edge"), &config);
    assert_eq!(hits(&out), vec![("mixed.ts".into(), 5), ("mixed.ts".into(), 17)], "{out:?}");
    assert!(out.iter().all(|f| f.severity == Severity::High), "{out:?}");
}
