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
//! - **Placeholders are not credentials.** A line carrying `example`,
//!   `sample`, `placeholder`, `changeme`, `your_` or a run of `x` is
//!   documenting a shape; a *value* carrying an angle bracket placeholder, a
//!   `${}` interpolation, a `process.env` reference or a provider's own test
//!   prefix is a template; and a value of one repeated character is filler. The
//!   two halves are deliberately different scopes: see [`LINE_PLACEHOLDER`] and
//!   [`VALUE_PLACEHOLDER`].
//! - **A development connection string is not a credential.** `postgres://
//!   postgres:postgres@localhost:54322` is in every Supabase repository on
//!   earth. A URI whose host is local, or whose password equals its username,
//!   or whose password is one of the dozen dev defaults, is not reported.
//! - **The generic entries stand behind an entropy gate**, see
//!   [`entropy::looks_random`], **and behind the named ones**: a generic entry
//!   does not report a value a provider entry already claimed on that line, so
//!   one committed key is one finding rather than two.
//!
//! **What it never prints.** Evidence is the provider and a mask: the first
//! four characters, an ellipsis, and the length. The finding's anchor carries
//! a blake3 prefix of the value rather than the value, so two different keys on
//! one line stay two findings and neither is quoted. Nothing downstream, a
//! report, a SARIF file, a pull request comment, ever holds the secret.
//!
//! **Unmeasured on the corpus.** Zero findings across the 2674 indexed files of
//! the five repositories the engine is measured against, so the spec 10.2
//! precision gate has no sample here and the rule ships on fixture evidence.
//! The zero was checked rather than assumed (see the precision report): it is a
//! rule that ran over every line and found nothing, not a rule that never ran.
//! A locked rule with no sample is the one place that would have STOPPED for
//! the founder had it produced a false positive, and it produced no findings at
//! all.
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

use crate::{clean_files, finding_at, line_span, Language, Rule, RuleContext, Scope, ALL};

pub struct SecretExposed;

const FIX: &str =
    "Revoke the credential now, move it to an environment variable or secret store, and purge it from git history";

/// Words that make the whole line a template. A repository writes `example`,
/// `your_token` or a run of `x` beside the shape it is documenting, and the
/// word lands on the name as often as on the value, so this one is read over
/// the line. It costs the rule a real key that shares a line with the word
/// `sample`, and that is the right way round for a rule nobody can turn off.
const LINE_PLACEHOLDER: &str = r"(?i)(example|sample|placeholder|changeme|your[_-]|xxx+)";

/// Structure that makes the *value* a template: an angle bracket placeholder,
/// a `${}` interpolation, an environment reference.
///
/// These three were read over the whole line too, which is a different claim
/// and a wrong one: they are syntax that appears everywhere near a value
/// without saying anything about it. Line-wide they hid a key in a tag's props
/// (`<GoogleMap apiKey="AIza..." />`), a key behind a generic parameter
/// (`useState<string>("AKIA...")`), a key in a template literal that
/// interpolates something else, and the fallback in
/// `process.env.KEY || "AIza..."`, which is the value that ships when the
/// variable is unset. Read over the value, each one still excuses what it
/// should: `mongodb://user:<password>@host` and
/// `postgres://user:${PGPASSWORD}@host` are shapes, not credentials.
///
/// `sk_test_` and its neighbours live here rather than in the word list because
/// a provider's own test prefix says the value is a sandbox key wherever it
/// appears, and no line needs to mention it.
const VALUE_PLACEHOLDER: &str = r"(?i)(<[^>]+>|\$\{|process\.env|sk_test_|pk_test_|_test_)";

/// What the name half of an assignment has to look like for an otherwise
/// unremarkable JWT to be read as a credential.
const CREDENTIAL_NAME: &str = r"(?i)(secret|token|key|password|passwd|pwd|credential|auth)";

/// The Supabase role that bypasses row level security.
const SERVICE_ROLE: &str = "service_role";

/// The `role` claims that make a JWT a credential on their own.
///
/// This is a list of names rather than "anything that is not `anon` or
/// `authenticated`", which is what it used to be and which read every
/// application's own vocabulary as privileged: a session token with
/// `"role": "viewer"`, `"member"` or `"customer"` was a locked High finding on
/// a fixture. A role outside this list falls through to the credential name
/// gate, so `authToken = "<viewer token>"` is still reported and
/// `viewerSession = "<the same token>"` is not.
const PRIVILEGED_ROLES: [&str; 7] =
    [SERVICE_ROLE, "supabase_admin", "supabase_auth_admin", "admin", "superuser", "owner", "root"];

/// The longest line the rule looks at. A generated bundle or a base64 asset
/// inlined into a source file is one enormous line and holds no credential
/// anybody typed; a real one lives on a line a person wrote.
const MAX_LINE: usize = 2000;

/// Passwords a URI carries in a development compose file or a local Supabase
/// stack. None of them protects anything.
const DEV_PASSWORDS: [&str; 12] =
    ["password", "postgres", "root", "admin", "secret", "test", "guest", "user", "mysql", "redis", "dev", "local"];

const LOCAL_HOSTS: [&str; 6] = ["localhost", "127.0.0.1", "0.0.0.0", "::1", "host.docker.internal", "db"];

fn line_placeholder() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(LINE_PLACEHOLDER).expect("the line placeholder pattern compiles"))
}

fn value_placeholder() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(VALUE_PLACEHOLDER).expect("the value placeholder pattern compiles"))
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

/// The shortest run of key material a private key header is allowed to be
/// followed by on its own line, where the material is what a written `\n`
/// escape separates from the header, and on the next source line, where a run
/// this long at the start of a line is a PEM body and not an identifier.
const SAME_LINE_MATERIAL: usize = 16;
const NEXT_LINE_MATERIAL: usize = 40;

/// The two headers OpenSSL writes between the `BEGIN` line of a legacy
/// encrypted PEM and its base64 body. The body is three lines down rather than
/// one, so a check that only reads the next source line finds one of these and
/// would otherwise call an encrypted private key not a key at all. Either line
/// is material: neither appears anywhere but in a PEM, and the `DEK-Info`
/// initialisation vector is what tells two encrypted keys apart.
const ENCRYPTED_PEM_HEADERS: [&str; 2] = ["Proc-Type:", "DEK-Info:"];

/// The key material a private key header is followed by: the base64 run after a
/// `\n` escape on the same line, the run that opens the next source line, or
/// one of the two headers a legacy encrypted PEM writes in front of its body.
///
/// The header on its own is not a key. It is the string a program compares
/// against (`key.startsWith("-----BEGIN RSA PRIVATE KEY-----")`) or strips out
/// (`pem.replace("-----BEGIN PRIVATE KEY-----", "")`), and it is character for
/// character the same in every repository, so a finding anchored on it gives
/// every private key in a file one id and one decision. The material is both
/// what makes it a key and what tells two of them apart.
fn key_material<'a>(rest_of_line: &'a str, next_line: Option<&'a str>) -> Option<&'a str> {
    let same = base64_run(rest_of_line);
    if same.len() >= SAME_LINE_MATERIAL {
        return Some(same);
    }
    let next_line = next_line?;
    let header = next_line.trim_matches(|c: char| c.is_whitespace() || "\"'`,+".contains(c));
    if ENCRYPTED_PEM_HEADERS.iter().any(|h| header.starts_with(h)) {
        return Some(header);
    }
    let next = base64_run(next_line);
    (next.len() >= NEXT_LINE_MATERIAL).then_some(next)
}

/// The base64 run at the front of a string, after the separators a header and
/// its material can have between them inside a source literal: whitespace, the
/// quote that closes the string, a comma, the `+` of a concatenation, and the
/// two characters of a written `\n`.
fn base64_run(s: &str) -> &str {
    let mut rest = s;
    while let Some(next) = rest
        .strip_prefix("\\r\\n")
        .or_else(|| rest.strip_prefix("\\n"))
        .or_else(|| rest.strip_prefix(|c: char| c.is_whitespace() || "\"'`,+".contains(c)))
    {
        rest = next;
    }
    let end =
        rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')).unwrap_or(rest.len());
    &rest[..end]
}

/// The gates, in the order they cost least to run. `start` is where the whole
/// match begins, so the JWT arm can ask what the value was assigned to.
fn reported(provider: &str, value: &str, line: &str, start: usize) -> bool {
    if is_filler(value) || line_placeholder().is_match(line) || value_placeholder().is_match(value) {
        return false;
    }
    if value.contains("://") && weak_uri_credential(value) {
        return false;
    }
    match provider {
        patterns::SUPABASE_SERVICE_ROLE => jwt::role(value).as_deref() == Some(SERVICE_ROLE) && !jwt::expired(value),
        patterns::JWT if jwt::expired(value) => false,
        // A named privileged role is a credential wherever it sits. Supabase's
        // two public roles never are, whatever the line calls them:
        // `SUPABASE_ANON_KEY` is a credential-shaped name holding a value that
        // is meant to be in the browser. The service role token belongs to the
        // entry above, so this one steps aside rather than report the same line
        // twice. Everything else is an application's own word for a user, and
        // is a credential only when the name on the line says so.
        patterns::JWT => match jwt::role(value).as_deref() {
            Some("anon" | "authenticated" | SERVICE_ROLE) => false,
            Some(role) if PRIVILEGED_ROLES.contains(&role) => true,
            _ => credential_name().is_match(&line[..start]),
        },
        patterns::BASIC_AUTH => decodes_to_a_pair(value),
        p if patterns::GENERIC.contains(&p) => entropy::looks_random(value),
        _ => true,
    }
}

fn scan(rule: &SecretExposed, file: &ParsedFile) -> Vec<Finding> {
    let compiled = patterns::compiled();
    // The whole file, because one entry reads past the line it matched on: a
    // private key header is regularly the last thing on its line and its
    // material the first thing on the next.
    let lines: Vec<&str> = file.source.lines().collect();
    let mut out = Vec::new();
    // One finding per line per provider: a key repeated twice on one line is one
    // decision to make, and two providers on one line are two.
    let mut seen: HashSet<(u32, &'static str)> = HashSet::new();
    for (i, line) in lines.iter().copied().enumerate() {
        let lineno = i as u32 + 1;
        if line.len() > MAX_LINE || !compiled.set.is_match(line) {
            continue;
        }
        // What a named provider has already claimed on this line. A generic
        // entry that would report the same characters steps aside: `apiKey =
        // "AIza..."` is one committed key and one decision, and reporting it as
        // both a Google API key and an API key assignment puts two locked High
        // findings on somebody's build for it. The named entries all sit ahead
        // of the generic ones in the table and `matches` yields them in table
        // order, so the claim is always made before it is checked.
        let mut claimed: Vec<&str> = Vec::new();
        for idx in compiled.set.matches(line) {
            let pattern = &patterns::PATTERNS[idx];
            for caps in compiled.at(idx).captures_iter(line) {
                let whole = caps.get(0).map(|m| m.range()).unwrap_or(0..0);
                let start = whole.start;
                let value = if pattern.provider == patterns::PRIVATE_KEY {
                    match key_material(&line[whole.end..], lines.get(i + 1).copied()) {
                        Some(material) => material,
                        None => continue,
                    }
                } else {
                    patterns::value_of(&caps)
                };
                if value.is_empty() || !reported(pattern.provider, value, line, start) {
                    continue;
                }
                let generic = patterns::GENERIC.contains(&pattern.provider);
                if generic && claimed.iter().any(|c| c.contains(value) || value.contains(c)) {
                    continue;
                }
                if !seen.insert((lineno, pattern.provider)) {
                    continue;
                }
                if !generic {
                    claimed.push(value);
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
    /// Every language: a key is a string literal, and a key committed in a PHP
    /// config or a Python settings module is the same leak as one committed in
    /// a TypeScript module. The scan reads the line's text, not a node kind the
    /// TypeScript grammar owns.
    fn languages(&self) -> &'static [Language] {
        ALL
    }

    fn run(&self, ctx: &RuleContext) -> anyhow::Result<Vec<Finding>> {
        Ok(clean_files(ctx).flat_map(|file| scan(self, file)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private key header is a key when something follows it. The three
    /// clean shapes below are the ones a repository actually writes: a header
    /// compared against, a header stripped out, and a header at the end of a
    /// line whose next line is ordinary code.
    #[test]
    fn key_material_is_what_follows_the_header_and_a_bare_header_is_not_a_key() {
        let body = "MIIEowIBAAKCAQEAsynthetic0fixture0material0for0locrin0only0A1b2";
        // The separator these cases pass is the two character `\n` escape, the
        // way a PEM is written inside a source literal, so what they exercise is
        // `base64_run` stepping over the escape. A real newline never reaches
        // this function on the same line: the scan splits the file on it, and
        // the material after one arrives as the next source line, which is the
        // case below.
        assert_eq!(key_material(&format!("\\n{body}\\n-----END"), None), Some(body), "after a written newline");
        assert_eq!(key_material("\";", Some(&format!("  \"{body}\","))), Some(body), "on the next source line");
        assert_eq!(key_material("\", \"\")", Some("export const looksLikeAKey = false;")), None, "stripped out");
        assert_eq!(key_material("\");", Some("declare const pem: string;")), None, "compared against");
        assert_eq!(key_material("", None), None, "the last line of a file");
        // Sixteen characters on the same line is material; a short identifier
        // opening the next line is not.
        assert_eq!(key_material("\\nMIIEowIBAAKCAQEA", None).map(str::len), Some(16));
        assert_eq!(key_material("\\nMIIEowIBAAKCAQE", None), None, "fifteen characters is not a body");
        assert_eq!(key_material("\";", Some("someLongIdentifierName.method();")), None, "an identifier, not a body");
        // A legacy encrypted PEM puts two headers of its own between the BEGIN
        // line and the base64 body, so the next source line is one of them.
        assert_eq!(
            key_material("", Some("Proc-Type: 4,ENCRYPTED")),
            Some("Proc-Type: 4,ENCRYPTED"),
            "an encrypted PEM"
        );
        assert_eq!(
            key_material("", Some("  \"DEK-Info: DES-EDE3-CBC,8F3A2B1C4D5E6F70\"")),
            Some("DEK-Info: DES-EDE3-CBC,8F3A2B1C4D5E6F70"),
            "the DEK-Info line, unquoted"
        );
        assert_eq!(key_material("", Some("// Proc-Type is what openssl writes")), None, "prose about the header");
    }
}
