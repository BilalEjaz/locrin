mod common;

use common::{fixture, hits, run_on};
use locrin_core::config::{Config, Framework};
use locrin_core::finding::{Category, Confidence, Severity};
use locrin_rules::supabase::rls::SupabaseTableWithoutRls;
use locrin_rules::supabase::service_role::SupabaseServiceRoleInClient;

const CLIENT_FIX: &str = "Move this call behind a server (edge function, API route) and use the anon key on the client; the service role bypasses row-level security";

/// The config a repository writes when its server code lives somewhere the
/// defaults do not name. The list replaces the defaults rather than adding to
/// them, which is what makes the second half of `an_overridden_server_paths_list_replaces_the_defaults` true.
fn server_paths(globs: &[&str]) -> Config {
    let framework = Framework { server_paths: globs.iter().map(|g| g.to_string()).collect(), ..Framework::default() };
    Config { framework, ..Config::default() }
}

#[test]
fn a_client_file_holding_the_service_role_key_is_flagged_once_per_line() {
    let out = run_on(Box::new(SupabaseServiceRoleInClient), &fixture("supabase", "flag"), &Config::default());
    assert_eq!(hits(&out), vec![("app/(tabs)/home.tsx".to_string(), 5), ("app/(tabs)/home.tsx".to_string(), 11)]);
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec![
            "service-role key referenced in client code (SUPABASE_SERVICE_ROLE_KEY)",
            "service-role key referenced in client code (a service_role JWT)",
        ]
    );
    assert!(out.iter().all(|f| f.fix == CLIENT_FIX), "{:?}", out.first());
    // The evidence names the shape and never the token: a finding is read in a
    // pull request comment and in a SARIF file, and neither is a place to
    // publish a live key.
    assert!(out.iter().all(|f| !f.evidence.contains("eyJ")), "{:?}", out.last());
}

/// Spec 7.1 metadata, from the plan's global constraints.
#[test]
fn every_service_role_finding_carries_the_security_metadata() {
    let out = run_on(Box::new(SupabaseServiceRoleInClient), &fixture("supabase", "flag"), &Config::default());
    assert!(!out.is_empty());
    assert!(
        out.iter().all(|f| f.category == Category::Security
            && f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A01:2021")
            && f.cwe.as_deref() == Some("CWE-284")),
        "{:?}",
        out.first()
    );
}

#[test]
fn server_code_the_anon_key_and_a_comment_are_not_client_exposure() {
    let out = run_on(Box::new(SupabaseServiceRoleInClient), &fixture("supabase", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

/// The default list exempts a test file and says nothing about `tools/`. A
/// repository that names `tools/` replaces the list, so its test files stop
/// being exempt: the config is the whole answer, not an addition to the
/// defaults.
///
/// `src/env.ts` is silent in all three runs and is named in none of them: it
/// declares the binding, which is the corpus tightening, and the assertions
/// below fail if that stops holding.
#[test]
fn an_overridden_server_paths_list_replaces_the_defaults() {
    let root = fixture("supabase", "edge");
    let default = run_on(Box::new(SupabaseServiceRoleInClient), &root, &Config::default());
    assert_eq!(hits(&default), vec![("tools/seed.ts".to_string(), 5)], "{default:?}");

    let overridden = run_on(Box::new(SupabaseServiceRoleInClient), &root, &server_paths(&["tools/**"]));
    assert_eq!(hits(&overridden), vec![("src/__tests__/keys.test.ts".to_string(), 6)], "{overridden:?}");

    let both = run_on(Box::new(SupabaseServiceRoleInClient), &root, &server_paths(&["tools/**", "**/*.test.*"]));
    assert!(both.is_empty(), "{both:?}");
}

#[test]
fn a_created_table_that_no_migration_locks_down_is_flagged_at_its_create_line() {
    let out = run_on(Box::new(SupabaseTableWithoutRls), &fixture("supabase", "flag"), &Config::default());
    assert_eq!(
        hits(&out),
        vec![
            ("supabase/migrations/20260101000000_init.sql".to_string(), 9),
            ("supabase/migrations/20260202000000_orders.sql".to_string(), 1),
        ],
        "{out:?}"
    );
    let evidence: Vec<&str> = out.iter().map(|f| f.evidence.as_str()).collect();
    assert_eq!(
        evidence,
        vec![
            "table audit_log is created without row-level security",
            "table Orders is created without row-level security",
        ]
    );
    let fixes: Vec<&str> = out.iter().map(|f| f.fix.as_str()).collect();
    assert_eq!(
        fixes,
        vec![
            "Add ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY and policies in the same migration",
            "Add ALTER TABLE Orders ENABLE ROW LEVEL SECURITY and policies in the same migration",
        ]
    );
    assert!(
        out.iter().all(|f| f.category == Category::Security
            && f.severity == Severity::High
            && f.confidence == Confidence::High
            && f.owasp.as_deref() == Some("A01:2021")
            && f.cwe.as_deref() == Some("CWE-284")),
        "{:?}",
        out.first()
    );
}

/// A later migration counts, a table in another schema does not, and a
/// commented-out `create table` is not a create.
#[test]
fn rls_enabled_in_a_later_migration_and_a_non_public_table_are_both_quiet() {
    let out = run_on(Box::new(SupabaseTableWithoutRls), &fixture("supabase", "clean"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}

/// A repository with no `supabase/migrations` directory is not an error and not
/// a finding; the rule runs on every repository the engine checks.
#[test]
fn a_repository_with_no_migrations_produces_nothing() {
    let out = run_on(Box::new(SupabaseTableWithoutRls), &fixture("supabase", "edge"), &Config::default());
    assert!(out.is_empty(), "{out:?}");
}
