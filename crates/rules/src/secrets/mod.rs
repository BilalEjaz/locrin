//! Flags a live credential committed to source. This is the one rule the config
//! cannot turn off or turn down (spec 4.3): a repository can accept a secret
//! into the baseline, where the acceptance is written down with a reason and a
//! date, but it cannot quietly agree to stop looking.
//!
//! **What it reads.** Every line of every parsed file, comments included. A key
//! commented out is a key that was pushed, and git remembers it whether or not
//! the parser thinks the line runs. The pattern table in [`patterns`] holds
//! more than a hundred provider shapes; a line the [`regex::RegexSet`] rejects
//! costs one pass and nothing else.
//!
//! **Why precision matters more here than anywhere else.** A locked rule at
//! High severity blocks. A false positive is therefore not a nuisance, it is a
//! build somebody cannot unblock without editing a baseline, so every gate
//! below exists to make the rule under-report rather than guess:
//!
//! - **A shape or a name, never a length.** Thirty-two hex digits are a
//!   credential when the line says Mailgun and a checksum when it does not. The
//!   entries with no distinctive prefix all carry the provider's name in the
//!   expression.
//! - **Public identifiers are excluded by construction.** A Supabase anon JWT,
//!   a Stripe publishable key, a Mapbox `pk.` token and a Sentry DSN without
//!   its secret half are all meant to ship to a browser. The table does not
//!   match them, and the anon JWT, which shares its shape exactly with the
//!   service role key, is separated by decoding the payload in [`jwt`].
//! - **Placeholders are not credentials.** Anything with `example`, `sample`,
//!   `placeholder`, `changeme`, `your_`, a run of `x`, an angle bracket
//!   placeholder, a `${}` interpolation or a `process.env` reference anywhere
//!   on the line is a template, and a value of one repeated character is a
//!   filler. The check runs over the whole line rather than the match, which
//!   costs the rule a real secret sharing a line with the word `example` and
//!   is the right way round.
//! - **A development connection string is not a credential.** `postgres://
//!   postgres:postgres@localhost:54322` is in every Supabase repository on
//!   earth. A URI whose host is local, or whose password equals its username,
//!   or whose password is one of the dozen dev defaults, is not reported.
//! - **The three generic entries stand behind an entropy gate.** See
//!   [`entropy::looks_random`].
//!
//! **What it never prints.** Evidence is the provider and a mask: the first
//! four characters, an ellipsis, and the length. The finding's anchor carries
//! a blake3 prefix of the value rather than the value, so two different keys on
//! one line stay two findings and neither is quoted. Nothing downstream, a
//! report, a SARIF file, a pull request comment, ever holds the secret.
//!
//! **Where it under-reports on purpose.** A secret built at runtime from
//! fragments, a base64 blob with no provider shape, a credential in a file the
//! engine does not parse (`.env`, JSON, YAML: these are not source files and
//! the walker never reaches them), and any provider not yet in the table. The
//! table is the unit of growth: a new provider is one entry and two fixture
//! lines.

pub mod entropy;
pub mod jwt;
pub mod patterns;

use std::collections::HashSet;
use std::sync::OnceLock;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use locrin_core::finding::{Category, Confidence, Finding, Severity};
use locrin_core::parse::ParsedFile;
use regex::Regex;

use crate::{clean_files, finding_at, line_span, Rule, RuleContext, Scope};

pub struct SecretExposed;

const FIX: &str =
    "Revoke the credential now, move it to an environment variable or secret store, and purge it from git history";

/// Anything on the line that says the value is a template rather than a
/// credential.
const PLACEHOLDER: &str = r"(?i)(example|sample|placeholder|changeme|your[_-]|xxx+|<[^>]+>|\$\{|process\.env)";

/// What the name half of an assignment has to look like for an otherwise
/// unremarkable JWT to be read as a credential.
const CREDENTIAL_NAME: &str = r"(?i)(secret|token|key|password|passwd|pwd|credential|auth)";

/// The longest line the rule looks at. A generated bundle or a base64 asset
/// inlined into a source file is one enormous line and holds no credential
/// anybody typed; a real one lives on a line a person wrote.
const MAX_LINE: usize = 2000;

/// Passwords a URI carries in a development compose file or a local Supabase
/// stack. None of them protects anything.
const DEV_PASSWORDS: [&str; 12] =
    ["password", "postgres", "root", "admin", "secret", "test", "guest", "user", "mysql", "redis", "dev", "local"];

const LOCAL_HOSTS: [&str; 6] = ["localhost", "127.0.0.1", "0.0.0.0", "::1", "host.docker.internal", "db"];

fn placeholder() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(PLACEHOLDER).expect("the placeholder pattern compiles"))
}

fn credential_name() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(CREDENTIAL_NAME).expect("the credential name pattern compiles"))
}

/// The only form of a credential that ever leaves this module: the first four
/// characters, an ellipsis, and how long the whole thing was. Four is enough to
/// tell two keys apart in a report and to recognise the provider prefix; it is
/// not enough to use.
pub fn mask(value: &str) -> String {
    let head: String = value.chars().take(4).collect();
    format!("{head}...({} chars)", value.chars().count())
}

/// Filler rather than a credential. Two bits of surprise per character is far
/// below anything a generator produces (a hex digest carries close to four, a
/// base62 token more) and far above nothing, which is what
/// `aaaaaaaaaaaaaaaa`, `xxxx-xxxx`, `pul-000...000` and the nil UUID that every
/// default config in the world writes as
/// `00000000-0000-0000-0000-000000000000` carry. A prefix on the front does not
/// rescue them, which is why this measures the value rather than comparing its
/// characters.
///
/// This runs on every pattern, unlike [`entropy::looks_random`], which is the
/// much stricter gate the three generic entries stand behind.
const FILLER_BITS: f64 = 2.0;

fn is_filler(value: &str) -> bool {
    entropy::shannon(value) < FILLER_BITS
}

/// Whether a connection string's password is a development default rather than
/// a credential: a local host, a password equal to the username, or one of the
/// words every compose file uses.
fn weak_uri_credential(value: &str) -> bool {
    let Some((_, rest)) = value.split_once("://") else {
        return false;
    };
    let Some((creds, host)) = rest.split_once('@') else {
        return false;
    };
    let (user, pass) = creds.split_once(':').unwrap_or(("", creds));
    let hostname = host.split(['/', ':', '?']).next().unwrap_or("");
    LOCAL_HOSTS.contains(&hostname)
        || (!user.is_empty() && pass.eq_ignore_ascii_case(user))
        || DEV_PASSWORDS.iter().any(|d| pass.eq_ignore_ascii_case(d))
}

/// Whether a `Basic` header's payload decodes to a `user:password` pair. An
/// arbitrary base64 run in a header is more often an encoded body or an asset;
/// a credential decodes to two printable halves.
fn decodes_to_a_pair(value: &str) -> bool {
    let Ok(bytes) = STANDARD.decode(value) else {
        return false;
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return false;
    };
    let Some((user, pass)) = text.split_once(':') else {
        return false;
    };
    !user.is_empty()
        && !pass.is_empty()
        && text.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        && !pass.contains(':')
}

/// The gates, in the order they cost least to run. `start` is where the whole
/// match begins, so the JWT arm can ask what the value was assigned to.
fn reported(provider: &str, value: &str, line: &str, start: usize) -> bool {
    if is_filler(value) || placeholder().is_match(line) {
        return false;
    }
    if value.contains("://") && weak_uri_credential(value) {
        return false;
    }
    match provider {
        patterns::SUPABASE_SERVICE_ROLE => jwt::role(value).as_deref() == Some("service_role"),
        // A privileged role is a credential wherever it sits. A role that is
        // not privileged is a client key and never reported, whatever the line
        // calls it: `SUPABASE_ANON_KEY` is a credential-shaped name holding a
        // value that is meant to be public. The service role token belongs to
        // the entry above, so this one steps aside for it rather than reporting
        // the same line twice.
        patterns::JWT => match jwt::role(value).as_deref() {
            Some("anon") | Some("authenticated") | Some("service_role") => false,
            Some(_) => true,
            None => credential_name().is_match(&line[..start]),
        },
        patterns::BASIC_AUTH => decodes_to_a_pair(value),
        p if patterns::GENERIC.contains(&p) => entropy::looks_random(value),
        _ => true,
    }
}

fn scan(rule: &SecretExposed, file: &ParsedFile) -> Vec<Finding> {
    let compiled = patterns::compiled();
    let mut out = Vec::new();
    // One finding per line per provider: a key repeated twice on one line is one
    // decision to make, and two providers on one line are two.
    let mut seen: HashSet<(u32, &'static str)> = HashSet::new();
    for (i, line) in file.source.lines().enumerate() {
        let lineno = i as u32 + 1;
        if line.len() > MAX_LINE || !compiled.set.is_match(line) {
            continue;
        }
        for idx in compiled.set.matches(line) {
            let pattern = &patterns::PATTERNS[idx];
            for caps in compiled.regexes[idx].captures_iter(line) {
                let value = patterns::value_of(&caps);
                let start = caps.get(0).map(|m| m.start()).unwrap_or(0);
                if value.is_empty() || !reported(pattern.provider, value, line, start) {
                    continue;
                }
                if !seen.insert((lineno, pattern.provider)) {
                    continue;
                }
                let anchor = format!("{}\x1f{}", pattern.provider, &blake3::hash(value.as_bytes()).to_hex()[..8]);
                let evidence = format!("{} credential: {}", pattern.provider, mask(value));
                let mut finding = finding_at(rule, &file.rel, line_span(file, lineno), &anchor, &evidence, FIX);
                finding.owasp = Some("A02:2021".to_string());
                finding.cwe = Some("CWE-798".to_string());
                out.push(finding);
            }
        }
    }
    out
}

impl Rule for SecretExposed {
    fn id(&self) -> &'static str {
        "secret-exposed"
    }
    fn description(&self) -> &'static str {
        "A provider credential committed to source, which is live until it is revoked"
    }
    fn scope(&self) -> Scope {
        Scope::File
    }
    fn category(&self) -> Category {
        Category::Security
    }
    fn default_severity(&self) -> Severity {
        Severity::High
    }
    /// High: every entry either matches a shape no other string takes or
    /// carries the provider's own name, and the generic three are gated on
    /// entropy. The rule would rather miss a key than name one that is not.
    fn confidence(&self) -> Confidence {
        Confidence::High
    }
    /// Locked (spec 4.3). The config cannot disable this rule or lower its
    /// severity; a repository that has decided about a particular finding says
    /// so in the baseline.
    fn locked(&self) -> bool {
        true
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}
