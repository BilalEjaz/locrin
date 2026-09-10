//! Flags a table a Supabase migration creates and no migration locks down.
//!
//! Supabase exposes every table in the `public` schema through PostgREST, which
//! any browser holding the anon key can call. Row-level security is what stands
//! between that call and the rows: a table with policies returns what the
//! policies allow, and a table without them returns everything, to everyone.
//! Postgres creates tables with row-level security off, so the missing
//! `ALTER TABLE ... ENABLE ROW LEVEL SECURITY` is not a weakened setting but an
//! absent one, and the table is open from the moment the migration runs.
//!
//! Graph scope, and the one other rule (with `vulnerable-dependency`) that
//! reads a file the engine does not parse. Both are deliberate: a migration is
//! SQL, which no language in the index covers, and the answer spans every
//! migration at once. A table created in January and secured in March is
//! secure, and neither of those two files says so on its own; only the set
//! does. The read goes through `ctx.root`, the documented exception to "rules
//! never touch the filesystem", because the process's working directory is not
//! the repository root under a hook or an editor.
//!
//! Which is also the scoping contract, and the CLI enforces it before the rule
//! is asked to run (see `rls_in_scope` in `crates/cli/src/run.rs`). A run
//! narrowed to a scope answers for the migrations only when the scope itself
//! names one: a diff whose git file list holds a `.sql` under
//! `supabase/migrations`, a path argument naming one, or a directory argument
//! the migrations lie under. Any other scope skips the rule outright, reading
//! no migration at all, because the findings would have been discarded after
//! being paid for: a migration is not a source file, so it is in no walk, no
//! index and no import neighbourhood. `--changed` is therefore never a run that
//! reports a table without row-level security, whatever was done to the
//! migrations: that scope is the index's watermark and the index holds source
//! files only, so it sees neither a `.sql` nor the lockfile.
//!
//! **Unmeasured on the corpus.** Zero findings across the five repositories,
//! and the zero was checked rather than assumed: FastLift's 35 migrations
//! create 33 tables and enable row-level security on all 33, so the rule ships
//! on fixture evidence with 33 correct answers behind it (see the precision
//! report).
//!
//! `locrin:allow` cannot suppress a finding from this rule. The marker is read
//! off the line a finding sits on, and for a file the run never parsed that
//! reading comes from the index's allow rows, which exist only for source
//! files: a `.sql` has none, so a comment in a migration suppresses nothing.
//! The baseline is the suppression path, which is the better one anyway: an
//! accepted table is written down with a reason, a date and an author.
//!
//! The reading is regex over `supabase/migrations/*.sql`, sorted by name, which
//! is how Supabase orders them. Comments are blanked before the regexes run, so
//! a commented-out `create table` left in a migration as a note is not read as
//! a create; string and dollar-quoted literals are left alone, because a
//! function body written in `$$ ... $$` holds real DDL.
//!
//! Blind spots:
//!
//! - **Only `public`.** A table qualified with any other schema is skipped.
//!   `auth.users` and `storage.objects` are Supabase's own, they are not
//!   exposed through PostgREST by default, and their policies are not this
//!   repository's to write. An unqualified name is `public`, because that is
//!   what the default `search_path` in a Supabase migration resolves to.
//! - **Migrations are the only source.** A table created outside them, in the
//!   dashboard or by an extension, is invisible, and so is a policy added the
//!   same way. The rule reports what the repository can be read to say.
//! - **Enabling anywhere counts, dropping never does.** A later migration that
//!   drops the table still leaves its `create` on record, so the rule would
//!   report a table that no longer exists. Reading a `drop table` would need
//!   the rule to model the order of every statement rather than the set of
//!   them, and a dropped table in a migration history is rare enough that the
//!   simpler answer is the better one.
//! - **`enable row level security` is the whole test.** `force row level
//!   security` without it, or a policy created against a table whose security
//!   was never enabled, reads as absent, which is correct: a policy on a table
//!   with row-level security off is not enforced.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use locrin_core::finding::{Category, Confidence, Finding, Severity, Span};
use regex::Regex;

use crate::{finding_at, Rule, RuleContext, Scope};

pub struct SupabaseTableWithoutRls;

/// The directory Supabase generates migrations into. Fixed rather than
/// configurable: it is the path `supabase db diff` writes and the path the CLI
/// applies from, and a repository that moved it has moved off the convention
/// the rule is reading.
const MIGRATIONS: &str = "supabase/migrations";

/// Whether a repository-relative path is a migration this rule reads.
///
/// The CLI asks before it runs the rule at all, and it asks about the paths a
/// narrowed scope names rather than about a walk: a scope holding no migration
/// could not report a finding against one, so the rule is dropped from the run
/// instead of reading every migration to have its answer discarded. See the
/// module doc, and `rls_in_scope` in `crates/cli/src/run.rs`.
///
/// Direct children only, because [`migrations`] reads the directory rather than
/// walking it, and the two have to agree about what a migration is.
pub fn is_migration(rel: &str) -> bool {
    let Some(name) = rel.strip_prefix(MIGRATIONS).and_then(|rest| rest.strip_prefix('/')) else {
        return false;
    };
    !name.contains('/') && std::path::Path::new(name).extension().is_some_and(|x| x.eq_ignore_ascii_case("sql"))
}

/// A schema-qualified name, either half unquoted or double-quoted. Postgres
/// folds an unquoted identifier to lower case and preserves a quoted one, which
/// is what [`normalise`] does.
const NAME: &str =
    r#"(?:(?P<schema>"[^"]+"|[A-Za-z_][A-Za-z0-9_$]*)\s*\.\s*)?(?P<name>"[^"]+"|[A-Za-z_][A-Za-z0-9_$]*)"#;

fn create_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"(?i)\bcreate\s+table\s+(?:if\s+not\s+exists\s+)?{NAME}")).unwrap())
}

fn enable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(r"(?i)\balter\s+table\s+(?:only\s+)?{NAME}\s+enable\s+row\s+level\s+security")).unwrap()
    })
}

/// An identifier as Postgres stores it: a quoted name keeps its case, an
/// unquoted one folds down. Two spellings of one table have to answer the same,
/// or a `create table Profiles` and an `alter table profiles` would look like
/// two tables.
fn normalise(raw: &str) -> String {
    match raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(quoted) => quoted.to_string(),
        None => raw.to_lowercase(),
    }
}

/// Whether a match's schema is one this rule speaks for: `public`, or none at
/// all, which resolves to `public` under a migration's default `search_path`.
fn is_public(schema: Option<&str>) -> bool {
    match schema {
        None => true,
        Some(s) => normalise(s) == "public",
    }
}

/// The text with every comment replaced by spaces of the same length, so byte
/// offsets and therefore line numbers are unchanged.
///
/// Line comments run to the newline, block comments nest (Postgres nests them,
/// unlike C), and neither is a comment inside a string. Single-quoted strings
/// and dollar-quoted bodies are skipped over rather than blanked: a `$$ ... $$`
/// function body holds statements the rule wants to read.
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    // A byte copy rather than an in-place edit of the string: every replacement
    // is a space and every replaced byte belongs to a comment, so the result is
    // valid UTF-8 and the same length, but saying so with `unsafe` would be
    // asking the reader to check the proof. A copy that fails to decode falls
    // back to the original, which can only under-strip.
    let mut buf = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        // A single-quoted literal, `''` escaping a quote inside it.
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\'' {
                    if bytes.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // A dollar-quoted body, `$$ ... $$` or `$tag$ ... $tag$`.
        if bytes[i] == b'$' {
            if let Some(tag) = dollar_tag(&text[i..]) {
                let end = text[i + tag.len()..].find(&tag).map(|p| i + tag.len() + p + tag.len());
                i = end.unwrap_or(bytes.len());
                continue;
            }
        }
        if bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'-') {
            while i < bytes.len() && bytes[i] != b'\n' {
                buf[i] = b' ';
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut depth = 0usize;
            while i < bytes.len() {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    buf[i] = b' ';
                    buf[i + 1] = b' ';
                    i += 2;
                    continue;
                }
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    buf[i] = b' ';
                    buf[i + 1] = b' ';
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                if bytes[i] != b'\n' {
                    buf[i] = b' ';
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    String::from_utf8(buf).unwrap_or_else(|_| text.to_string())
}

/// The opening tag of a dollar-quoted string at the start of `rest`, e.g. `$$`
/// or `$body$`, or `None` when the dollar is something else (a positional
/// parameter, part of an identifier).
fn dollar_tag(rest: &str) -> Option<String> {
    let inner: String = rest[1..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if rest[1 + inner.len()..].starts_with('$') {
        Some(format!("${inner}$"))
    } else {
        None
    }
}

/// The 1-based line a byte offset falls on.
fn line_of(text: &str, offset: usize) -> u32 {
    text[..offset].bytes().filter(|b| *b == b'\n').count() as u32 + 1
}

/// Where a table was created: the migration's rel and the line of the `create`.
struct Created {
    rel: String,
    line: u32,
}

/// Every `*.sql` under `supabase/migrations`, sorted by file name, which is the
/// order Supabase applies them in.
fn migrations(root: &Path) -> Vec<std::path::PathBuf> {
    let dir = root.join(MIGRATIONS);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sql")))
        .collect();
    out.sort();
    out
}

impl Rule for SupabaseTableWithoutRls {
    fn id(&self) -> &'static str {
        "supabase-table-without-rls"
    }
    fn description(&self) -> &'static str {
        "Supabase table created without row-level security"
    }
    fn scope(&self) -> Scope {
        Scope::Graph
    }
    fn category(&self) -> Category {
        Category::Security
    }
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    fn confidence(&self) -> Confidence {
        Confidence::High
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        // Insertion order is the file order, which is sorted, so the findings
        // come out in migration order on every run and every machine.
        let mut created: BTreeMap<String, Created> = BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        let mut enabled: BTreeSet<String> = BTreeSet::new();

        for path in migrations(ctx.root) {
            let rel = path.strip_prefix(ctx.root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            // A migration that cannot be read is skipped and said aloud. A
            // directory named `x.sql`, a file the process may not open, or one
            // holding bytes that are not UTF-8 must not stop a run: the rest of
            // the migrations still have an answer, and the reader is told which
            // one was left out. The rule runs once per run, so each warning is
            // printed once.
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("warning: supabase-table-without-rls could not read {rel}; skipping it");
                continue;
            };
            let text = strip_comments(&text);
            for c in create_re().captures_iter(&text) {
                if !is_public(c.name("schema").map(|m| m.as_str())) {
                    continue;
                }
                let name = normalise(c.name("name").unwrap().as_str());
                // The first create is the one that created it. A later
                // `create table if not exists` for the same name is the same
                // table, and reporting both would be two findings sharing an id.
                if created.contains_key(&name) {
                    continue;
                }
                let line = line_of(&text, c.get(0).unwrap().start());
                order.push(name.clone());
                created.insert(name, Created { rel: rel.clone(), line });
            }
            for c in enable_re().captures_iter(&text) {
                if !is_public(c.name("schema").map(|m| m.as_str())) {
                    continue;
                }
                enabled.insert(normalise(c.name("name").unwrap().as_str()));
            }
        }

        let mut out = Vec::new();
        for name in order {
            if enabled.contains(&name) {
                continue;
            }
            let at = &created[&name];
            // Columns are zero: the finding is about the statement, and a
            // `create table` runs over as many lines as the table has columns.
            let span = Span { start_line: at.line, start_col: 0, end_line: at.line, end_col: 0 };
            // The table name is the anchor, not the line text, so the finding
            // keeps its id when the migration above it changes or the column
            // list grows.
            let anchor = format!("table\x1f{name}");
            let mut f = finding_at(
                self,
                &at.rel,
                span,
                &anchor,
                &format!("table {name} is created without row-level security"),
                &format!("Add ALTER TABLE {name} ENABLE ROW LEVEL SECURITY and policies in the same migration"),
            );
            f.owasp = Some("A01:2021".to_string());
            f.cwe = Some("CWE-284".to_string());
            out.push(f);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names a `create table` in each spelling yields, with the schema
    /// filter applied, so the two halves of the reading are pinned apart from
    /// the file walking.
    fn creates(sql: &str) -> Vec<(String, u32)> {
        let text = strip_comments(sql);
        create_re()
            .captures_iter(&text)
            .filter(|c| is_public(c.name("schema").map(|m| m.as_str())))
            .map(|c| (normalise(c.name("name").unwrap().as_str()), line_of(&text, c.get(0).unwrap().start())))
            .collect()
    }

    fn enables(sql: &str) -> Vec<String> {
        let text = strip_comments(sql);
        enable_re()
            .captures_iter(&text)
            .filter(|c| is_public(c.name("schema").map(|m| m.as_str())))
            .map(|c| normalise(c.name("name").unwrap().as_str()))
            .collect()
    }

    #[test]
    fn every_create_spelling_is_read_and_a_foreign_schema_is_not() {
        assert_eq!(creates("create table profiles (id uuid);"), vec![("profiles".to_string(), 1)]);
        assert_eq!(creates("CREATE TABLE public.Profiles (id uuid);"), vec![("profiles".to_string(), 1)]);
        assert_eq!(creates("create table if not exists public.\"Orders\" (id uuid);"), vec![("Orders".to_string(), 1)]);
        assert_eq!(
            creates("create   table\n  if not exists\n  audit_log (id uuid);"),
            vec![("audit_log".to_string(), 1)],
            "the statement's line is where it starts"
        );
        assert!(creates("create table auth.users (id uuid);").is_empty());
        assert!(creates("create table storage.objects (id uuid);").is_empty());
        assert_eq!(creates("create table \"public\".notes (id uuid);"), vec![("notes".to_string(), 1)]);
    }

    #[test]
    fn every_enable_spelling_is_read() {
        assert_eq!(enables("alter table profiles enable row level security;"), vec!["profiles"]);
        assert_eq!(enables("ALTER TABLE ONLY public.Profiles ENABLE ROW LEVEL SECURITY;"), vec!["profiles"]);
        assert_eq!(enables("alter table public.\"Orders\"\n  enable row level security;"), vec!["Orders"]);
        assert!(enables("alter table auth.users enable row level security;").is_empty());
        assert!(
            enables("alter table profiles force row level security;").is_empty(),
            "forcing is not enabling; a table with security off enforces no policy"
        );
    }

    /// A commented-out statement is a note, not a create, and blanking it must
    /// not move any line number.
    #[test]
    fn comments_are_blanked_in_place_and_never_read_as_statements() {
        let sql = "-- create table public.legacy (id uuid);\ncreate table public.live (id uuid);\n";
        assert_eq!(creates(sql), vec![("live".to_string(), 2)]);

        let sql = "/* create table public.legacy (id uuid);\n   and more */\ncreate table public.live (id uuid);\n";
        assert_eq!(creates(sql), vec![("live".to_string(), 3)], "a block comment keeps its newlines");

        let sql = "/* outer /* inner */ still a comment */\ncreate table public.live (id uuid);\n";
        assert_eq!(creates(sql), vec![("live".to_string(), 2)], "Postgres nests block comments");

        assert_eq!(strip_comments(sql).len(), sql.len(), "blanking never changes a byte offset");
    }

    /// A comment marker inside a string is text, and a function body written in
    /// dollar quotes is code the rule reads.
    #[test]
    fn a_literal_is_not_a_comment() {
        let sql = "insert into notes (body) values ('-- not a comment');\ncreate table public.live (id uuid);\n";
        assert_eq!(creates(sql), vec![("live".to_string(), 2)]);

        let sql =
            "create function f() returns void as $$\n  create table public.inner_t (id uuid);\n$$ language sql;\n";
        assert_eq!(creates(sql), vec![("inner_t".to_string(), 2)], "a dollar-quoted body is read");

        let sql = "select 'it''s fine -- really';\ncreate table public.live (id uuid);\n";
        assert_eq!(creates(sql), vec![("live".to_string(), 2)], "'' escapes a quote inside a literal");
    }

    /// What the CLI asks before it decides to run the rule at all.
    #[test]
    fn a_migration_is_a_sql_file_directly_under_the_migrations_directory() {
        assert!(is_migration("supabase/migrations/0001_orders.sql"));
        assert!(is_migration("supabase/migrations/0001_orders.SQL"));
        assert!(!is_migration("supabase/migrations/archive/0001_orders.sql"), "the rule reads direct children only");
        assert!(!is_migration("supabase/migrations/README.md"));
        assert!(!is_migration("supabase/functions/orders/index.ts"));
        assert!(!is_migration("db/migrations/0001_orders.sql"));
        assert!(!is_migration("supabase/migrations"));
    }

    #[test]
    fn an_unquoted_name_folds_down_and_a_quoted_one_does_not() {
        assert_eq!(normalise("Profiles"), "profiles");
        assert_eq!(normalise("\"Profiles\""), "Profiles");
        assert_eq!(normalise("\"profiles\""), "profiles");
    }

    /// A migrations directory that is not there, and one holding a file the
    /// rule cannot decode, both have to leave the run standing.
    #[test]
    fn an_absent_directory_and_an_undecodable_file_do_not_stop_the_run() {
        let dir = std::env::temp_dir().join(format!("locrin-rls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(migrations(&dir).is_empty(), "no directory is no migrations");

        let migrations_dir = dir.join(MIGRATIONS);
        std::fs::create_dir_all(&migrations_dir).unwrap();
        std::fs::write(migrations_dir.join("0001_bytes.sql"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        std::fs::write(migrations_dir.join("0002_ok.sql"), "create table public.live (id uuid);\n").unwrap();
        std::fs::write(migrations_dir.join("0003_notes.txt"), "create table public.ignored (id uuid);\n").unwrap();
        assert_eq!(migrations(&dir).len(), 2, "only .sql files, sorted");

        let files = migrations(&dir);
        assert!(files[0].ends_with("0001_bytes.sql"), "{files:?}");
        assert!(std::fs::read_to_string(&files[0]).is_err(), "the fixture has to be undecodable to test the skip");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
