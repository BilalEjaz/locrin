//! The provider table: one entry per credential shape the rule can name.
//!
//! Every entry is a provider label and one regular expression. The expression
//! either matches the credential on its own (a prefixed token such as
//! `ghp_...`, which no other string is shaped like) or matches the credential
//! together with the name it is assigned to (a bare 32 hex digits is only a
//! Mailgun key when the line says Mailgun). Where the expression carries
//! context, the credential itself is the named group `v`; where it does not,
//! the whole match is the credential. [`value_of`] is the one place that
//! distinction is read, so no caller has to know which entries have a group.
//!
//! Two rules govern what goes in here:
//!
//! - **A shape no ordinary string can take, or a name.** A pattern with neither
//!   (thirty-two hex digits, a forty character base62 string) is not a pattern,
//!   it is a guess, and a guess in a locked rule is a build somebody cannot
//!   unblock. The [`GENERIC`] entries at the end are the exception and they pay
//!   for it with the entropy gate in [`super::entropy::looks_random`], and with
//!   stepping aside for any named entry that claimed the same value first.
//! - **Public identifiers are not credentials.** A Stripe publishable key, a
//!   Twilio account SID on its own, a Supabase anon JWT, a Sentry DSN without
//!   its secret half and a Mapbox `pk.` token are all meant to ship inside a
//!   client bundle. None of them is in this table, and the two that share a
//!   shape with a real credential (the anon JWT, the `pk.` token) are excluded
//!   by construction rather than by a filter that could be forgotten.
//!
//! The example values in the fixtures are synthetic: they follow each
//! provider's documented format and none of them is, or ever was, a live
//! credential.

use std::sync::OnceLock;

use regex::{Regex, RegexSet};

/// One credential shape: what to call it, and how to find it.
pub struct Pattern {
    pub provider: &'static str,
    pub regex: &'static str,
}

/// The compiled table: a set for the fast reject, and the individual
/// expressions for pulling the value out of the line the set said matched.
pub struct Compiled {
    pub set: RegexSet,
    pub regexes: Vec<Regex>,
}

/// Compiles the table once per process. A bad expression here is a programming
/// error, not something a repository can provoke, so it panics with the
/// provider that owns it rather than degrading a locked rule into silence.
pub fn compiled() -> &'static Compiled {
    static COMPILED: OnceLock<Compiled> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let regexes = PATTERNS
            .iter()
            .map(|p| Regex::new(p.regex).unwrap_or_else(|e| panic!("secret pattern for {}: {e}", p.provider)))
            .collect();
        let set = RegexSet::new(PATTERNS.iter().map(|p| p.regex)).expect("every pattern compiles on its own first");
        Compiled { set, regexes }
    })
}

/// The credential inside a match: the named group `v` when the expression
/// carries context around the value, the whole match when it does not.
pub fn value_of<'t>(caps: &regex::Captures<'t>) -> &'t str {
    caps.name("v").or_else(|| caps.get(0)).map(|m| m.as_str()).unwrap_or("")
}

/// The delimiter a context entry requires between a credential's name and the
/// credential: the quote that closes the name if it has one, an assignment or a
/// colon, then an opening quote, and a closing quote on the far side of the
/// value.
///
/// This is the single most important string in the table, which is why it is
/// written once here and reached only through [`assigned!`]. The looser
/// separator it replaced (`["'\s:=]+`, a character class rather than a
/// sequence) accepted a bare identifier as the credential, so
/// `const expoToken = getExpoTokenFromSecureStore();` was a locked High finding
/// on a function call, and so was the type annotation
/// `expoToken: ExpoAccessTokenConfiguration`. A credential written into source
/// is a literal; anything else on the right of the equals sign is a name.
///
/// The closing quote matters as much as the opening one: without it a value
/// class runs to the end of the line and the mask reports a length nobody can
/// match against the key they are looking for.
///
/// The optional quote in front of the separator is the JSON style name. Without
/// it the separator had to sit against the last character of the name, and
/// `"Authorization": "Bearer ..."`, `"aws_secret_access_key": "..."` and every
/// other quoted key stopped matching: a headers object, a service account file
/// pasted into a module, a config map. It cannot reopen the identifier hole,
/// because it sits in front of the separator and the opening quote after the
/// separator is still required, so `"expoToken": getExpoTokenFromSecureStore()`
/// is a quoted name beside an identifier and not a finding.
pub const DELIMITER: &str = r#"["']?\s*[:=]\s*["'`]"#;

/// A context entry: the provider's own name, [`DELIMITER`], the value, and the
/// closing quote. Every entry that needs context is built here, so a new
/// provider cannot arrive with a separator of its own; the three argument form
/// is for a value written after a scheme word inside the quotes
/// (`Authorization: "Bearer <token>"`), where the scheme is part of the match
/// and no part of the credential.
///
/// Every half is a raw literal rather than an expression because `concat!` only
/// takes literals, which is also what stops a caller passing a runtime string.
macro_rules! assigned {
    ($context:literal, $value:literal) => {
        assigned!($context, "", $value)
    };
    ($context:literal, $in_quote:literal, $value:literal) => {
        concat!("(?i)", $context, r#"["']?\s*[:=]\s*["'`]"#, $in_quote, r#"(?P<v>"#, $value, r#")["'`]"#)
    };
}

/// The provider label of the JWT entry whose gate is the decoded role, and of
/// the one whose gate is the name it is assigned to. [`super`] special cases
/// both by label, so they are named here rather than spelled twice.
pub const SUPABASE_SERVICE_ROLE: &str = "Supabase service role key";
pub const JWT: &str = "JSON Web Token";
/// The label of the entry whose gate is a decodable `user:pass`.
pub const BASIC_AUTH: &str = "HTTP basic auth credential";
/// The label of the entry that reads a token out of an `Authorization` header.
pub const BEARER: &str = "bearer token literal";
/// The label of the entry whose match is a header rather than a credential. The
/// value [`super`] reports for it is the key material that follows, not the
/// header, which is the same string in every repository on earth.
pub const PRIVATE_KEY: &str = "Private key block";
/// The labels whose gate is [`super::entropy::looks_random`]. The bearer entry
/// is one of them because an `Authorization` header carries a repository's own
/// test doubles (`Bearer mockAccessTokenForTheSuite`) as readily as it carries
/// a real token, and the header shape says nothing about which it is.
pub const GENERIC: [&str; 4] = ["password assignment", "API key assignment", "secret assignment", BEARER];

/// The two entries that are allowed to name a value without [`DELIMITER`]
/// before it, because neither is an assignment: a key inside a connection
/// string, and a value after an HTTP scheme word in a header. Everything else
/// with a `v` group goes through [`assigned!`], which the test below enforces.
#[cfg(test)]
const NOT_ASSIGNMENTS: [&str; 2] = ["Azure storage account key", BASIC_AUTH];

pub const PATTERNS: &[Pattern] = &[
    // Cloud providers.
    Pattern { provider: "AWS access key ID", regex: r"\b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b" },
    Pattern {
        provider: "AWS secret access key",
        regex: assigned!(r#"aws[_-]?secret[_-]?access[_-]?key"#, r#"[A-Za-z0-9/+=]{40}"#),
    },
    Pattern {
        provider: "AWS session token",
        regex: assigned!(r#"aws[_-]?session[_-]?token"#, r#"[A-Za-z0-9/+=]{60,}"#),
    },
    Pattern { provider: "Google API key", regex: r"\bAIza[0-9A-Za-z_-]{35}\b" },
    Pattern { provider: "Google OAuth access token", regex: r"\bya29\.[0-9A-Za-z_-]{30,}\b" },
    Pattern { provider: "Google OAuth client secret", regex: r"\bGOCSPX-[0-9A-Za-z_-]{28}\b" },
    Pattern {
        provider: "Google service account private key id",
        regex: assigned!(r#""private_key_id""#, r#"[0-9a-f]{40}"#),
    },
    Pattern {
        provider: "Firebase Cloud Messaging server key",
        regex: r"\bAAAA[A-Za-z0-9_-]{7}:APA91b[A-Za-z0-9_-]{100,}",
    },
    Pattern { provider: "Azure storage account key", regex: r"AccountKey=(?P<v>[A-Za-z0-9+/=]{64,})" },
    Pattern {
        provider: "Azure client secret",
        regex: assigned!(r#"azure[^"'\n]{0,25}secret"#, r#"[A-Za-z0-9._~-]{32,}"#),
    },
    Pattern {
        provider: "Azure DevOps personal access token",
        regex: assigned!(r#"(?:azure[_-]?devops|vsts|ado)[^"'\n]{0,20}(?:pat|token)"#, r#"[a-z2-7]{52}"#),
    },
    Pattern { provider: "Alibaba Cloud access key ID", regex: r"\bLTAI[0-9A-Za-z]{12,20}\b" },
    Pattern {
        provider: "Alibaba Cloud access key secret",
        regex: assigned!(r#"alibaba[^"'\n]{0,25}secret"#, r#"[A-Za-z0-9]{30}"#),
    },
    Pattern { provider: "Tencent Cloud secret ID", regex: r"\bAKID[0-9A-Za-z]{32,}\b" },
    Pattern { provider: "Yandex Cloud API key", regex: r"\bAQVN[A-Za-z0-9_-]{35,}\b" },
    Pattern {
        provider: "IBM Cloud API key",
        regex: assigned!(r#"ibm[^"'\n]{0,25}(?:api)?key"#, r#"[A-Za-z0-9_-]{44}"#),
    },
    // Source hosting and CI.
    Pattern { provider: "GitHub token", regex: r"\bgh[pousr]_[A-Za-z0-9]{36,}\b" },
    Pattern { provider: "GitHub fine-grained token", regex: r"\bgithub_pat_[0-9a-zA-Z_]{40,}\b" },
    Pattern { provider: "GitLab personal access token", regex: r"\bglpat-[0-9A-Za-z_-]{20,}\b" },
    Pattern { provider: "GitLab pipeline trigger token", regex: r"\bglptt-[0-9a-f]{40}\b" },
    Pattern { provider: "GitLab runner registration token", regex: r"\bGR1348941[0-9A-Za-z_-]{20,}\b" },
    Pattern { provider: "GitLab deploy token", regex: r"\bgldt-[0-9A-Za-z_-]{20,}\b" },
    Pattern {
        provider: "Bitbucket app password",
        regex: assigned!(r#"bitbucket[^"'\n]{0,25}(?:password|token)"#, r#"[A-Za-z0-9]{20,}"#),
    },
    Pattern {
        provider: "CircleCI API token",
        regex: assigned!(r#"circle[_-]?ci[^"'\n]{0,20}token"#, r#"[0-9a-f]{40}"#),
    },
    Pattern { provider: "Buildkite agent token", regex: r"\bbkua_[0-9a-f]{40}\b" },
    Pattern { provider: "Travis CI token", regex: assigned!(r#"travis[^"'\n]{0,20}token"#, r#"[A-Za-z0-9_-]{22,}"#) },
    Pattern {
        provider: "Snyk API token",
        regex: assigned!(
            r#"snyk[_-]?(?:api[_-]?)?token"#,
            r#"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"#
        ),
    },
    Pattern { provider: "Postman API key", regex: r"\bPMAK-[0-9a-f]{24}-[0-9a-f]{34}\b" },
    Pattern { provider: "Pulumi access token", regex: r"\bpul-[0-9a-f]{40}\b" },
    Pattern { provider: "Doppler service token", regex: r"\bdp\.(?:pt|st|ct|sa)\.[A-Za-z0-9]{40,}\b" },
    Pattern { provider: "HashiCorp Vault token", regex: r"\bhvs\.[A-Za-z0-9_-]{24,}\b" },
    Pattern { provider: "Terraform Cloud token", regex: r"\b[A-Za-z0-9]{14}\.atlasv1\.[A-Za-z0-9_-]{50,}\b" },
    Pattern { provider: "1Password service account token", regex: r"\bops_[A-Za-z0-9_=-]{40,}\b" },
    // Chat, mail and social.
    Pattern { provider: "Slack token", regex: r"\bxox[abprs]-[0-9A-Za-z-]{12,}\b" },
    Pattern { provider: "Slack app-level token", regex: r"\bxapp-[0-9]-[A-Za-z0-9-]{20,}\b" },
    Pattern {
        provider: "Slack webhook URL",
        regex: r"https://hooks\.slack\.com/services/T[A-Za-z0-9_]{8,}/B[A-Za-z0-9_]{8,}/[A-Za-z0-9]{20,}",
    },
    Pattern {
        provider: "Discord bot token",
        regex: r"\b[MNO][A-Za-z0-9_-]{22,25}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,40}\b",
    },
    Pattern {
        provider: "Discord webhook URL",
        regex: r"https://discord(?:app)?\.com/api/webhooks/[0-9]{17,20}/[A-Za-z0-9_-]{60,}",
    },
    Pattern { provider: "Telegram bot token", regex: r"\b[0-9]{8,10}:AA[0-9A-Za-z_-]{33}\b" },
    Pattern { provider: "Twitter bearer token", regex: r"\bAAAAAAAAAAAAAAAAAAAAA[A-Za-z0-9%_-]{20,}\b" },
    Pattern {
        provider: "Twitter API secret",
        regex: assigned!(r#"twitter[_-]?(?:api|consumer)[_-]?secret"#, r#"[A-Za-z0-9]{40,50}"#),
    },
    Pattern { provider: "Facebook access token", regex: r"\bEAA[0-9A-Za-z]{30,}\b" },
    Pattern {
        provider: "Facebook app secret",
        regex: assigned!(r#"(?:facebook|fb)[_-]?app[_-]?secret"#, r#"[0-9a-f]{32}"#),
    },
    Pattern { provider: "Instagram access token", regex: r"\bIGQVJ[A-Za-z0-9_-]{50,}\b" },
    Pattern {
        provider: "LinkedIn client secret",
        regex: assigned!(r#"linkedin[^"'\n]{0,25}(?:client[_-]?)?secret"#, r#"[A-Za-z0-9]{16,}"#),
    },
    Pattern { provider: "SendGrid API key", regex: r"\bSG\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{40,}\b" },
    Pattern { provider: "Mailgun API key", regex: r"\bkey-[0-9a-f]{32}\b" },
    Pattern { provider: "Mailchimp API key", regex: r"\b[0-9a-f]{32}-us[0-9]{1,2}\b" },
    Pattern {
        provider: "Postmark server token",
        regex: assigned!(r#"postmark[^"'\n]{0,30}"#, r#"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"#),
    },
    Pattern { provider: "Resend API key", regex: r"\bre_[A-Za-z0-9]{8,}_[A-Za-z0-9]{20,}\b" },
    Pattern {
        provider: "Zendesk API token",
        regex: assigned!(r#"zendesk[^"'\n]{0,20}(?:api[_-]?)?token"#, r#"[A-Za-z0-9]{40}"#),
    },
    Pattern {
        provider: "Freshdesk API key",
        regex: assigned!(r#"freshdesk[^"'\n]{0,20}(?:api[_-]?)?key"#, r#"[A-Za-z0-9]{20,}"#),
    },
    Pattern { provider: "Intercom access token", regex: r"\bdG9r[A-Za-z0-9+/=]{40,}\b" },
    Pattern {
        provider: "Twilio auth token",
        regex: assigned!(r#"twilio[^"'\n]{0,25}(?:auth[_-]?token|secret)"#, r#"[0-9a-f]{32}"#),
    },
    // `SK` and thirty two hex digits is the API key SID: the identifier half of
    // a Twilio API key, not the secret. It is still worth a finding, because a
    // repository that has committed the SID has almost always committed the
    // secret beside it, but the label has to say which half it found.
    Pattern { provider: "Twilio API key SID", regex: r"\bSK[0-9a-f]{32}\b" },
    // Package registries.
    Pattern { provider: "npm access token", regex: r"\bnpm_[A-Za-z0-9]{36}\b" },
    Pattern { provider: "PyPI API token", regex: r"\bpypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{50,}\b" },
    Pattern { provider: "RubyGems API key", regex: r"\brubygems_[0-9a-f]{48}\b" },
    Pattern { provider: "NuGet API key", regex: r"\boy2[a-z0-9]{43}\b" },
    Pattern { provider: "Docker Hub personal access token", regex: r"\bdckr_pat_[A-Za-z0-9_-]{20,}\b" },
    Pattern { provider: "crates.io API token", regex: r"\bcio[A-Za-z0-9]{32}\b" },
    Pattern { provider: "JFrog Artifactory API key", regex: r"\bAKCp[A-Za-z0-9]{60,}\b" },
    // Hosting and platform.
    Pattern {
        provider: "Heroku API key",
        regex: assigned!(
            r#"heroku[_-]?api[_-]?key"#,
            r#"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"#
        ),
    },
    Pattern { provider: "DigitalOcean token", regex: r"\bdo[opr]_v1_[0-9a-f]{64}\b" },
    Pattern { provider: "Linode API token", regex: assigned!(r#"linode[_-]?(?:api[_-]?)?token"#, r#"[0-9a-f]{64}"#) },
    Pattern { provider: "Vultr API key", regex: assigned!(r#"vultr[_-]?api[_-]?key"#, r#"[A-Z0-9]{36}"#) },
    Pattern {
        provider: "Hetzner API token",
        regex: assigned!(r#"hetzner[^"'\n]{0,25}(?:token|key)"#, r#"[A-Za-z0-9]{64}"#),
    },
    Pattern {
        provider: "Cloudflare API token",
        regex: assigned!(r#"cloudflare[_-]?api[_-]?token"#, r#"[A-Za-z0-9_-]{40}"#),
    },
    Pattern {
        provider: "Cloudflare global API key",
        regex: assigned!(r#"cloudflare[^"'\n]{0,25}global[^"'\n]{0,20}key"#, r#"[0-9a-f]{37}"#),
    },
    Pattern {
        provider: "Vercel API token",
        regex: assigned!(r#"vercel[_-]?(?:api[_-]?)?token"#, r#"[A-Za-z0-9]{24}"#),
    },
    Pattern { provider: "Netlify personal access token", regex: r"\bnfp_[A-Za-z0-9]{36,}\b" },
    Pattern {
        provider: "Railway API token",
        regex: assigned!(
            r#"railway[_-]?(?:api[_-]?)?token"#,
            r#"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"#
        ),
    },
    Pattern { provider: "Render API key", regex: r"\brnd_[A-Za-z0-9]{28,}\b" },
    Pattern { provider: "Fly.io API token", regex: r"\bfo1_[A-Za-z0-9_-]{40,}\b" },
    Pattern { provider: "Expo access token", regex: assigned!(r#"expo[_-]?token"#, r#"[A-Za-z0-9_-]{20,}"#) },
    // Databases and backends.
    Pattern { provider: "MongoDB connection string", regex: r"mongodb(?:\+srv)?://[^:@\s/]+:[^@\s]{3,}@[^\s\x22']+" },
    Pattern { provider: "PostgreSQL connection string", regex: r"postgres(?:ql)?://[^:@\s/]+:[^@\s]{3,}@[^\s\x22']+" },
    Pattern { provider: "MySQL connection string", regex: r"mysql://[^:@\s/]+:[^@\s]{3,}@[^\s\x22']+" },
    Pattern { provider: "Redis connection string", regex: r"rediss?://[^:@\s/]*:[^@\s]{3,}@[^\s\x22']+" },
    Pattern { provider: "AMQP connection string", regex: r"amqps?://[^:@\s/]+:[^@\s]{3,}@[^\s\x22']+" },
    Pattern { provider: "PlanetScale password", regex: r"\bpscale_pw_[A-Za-z0-9_-]{32,}\b" },
    Pattern { provider: "PlanetScale service token", regex: r"\bpscale_tkn_[A-Za-z0-9_-]{32,}\b" },
    Pattern {
        provider: "Neon API key",
        regex: assigned!(r#"neon[^"'\n]{0,20}(?:api[_-]?key|token)"#, r#"[A-Za-z0-9]{32,}"#),
    },
    Pattern {
        provider: "Upstash REST token",
        regex: assigned!(r#"upstash[^"'\n]{0,30}token"#, r#"[A-Za-z0-9=_-]{40,}"#),
    },
    Pattern { provider: "Supabase personal access token", regex: r"\bsbp_[0-9a-f]{40}\b" },
    // Supabase's newer key format. `sb_publishable_` is the half a browser is
    // meant to hold and is not in this table; `sb_secret_` is the half that
    // reaches the API with row level security out of the way.
    Pattern { provider: "Supabase secret key", regex: r"\bsb_secret_[A-Za-z0-9_-]{20,}\b" },
    // The two JWT entries. Same shape, different gate: the first is flagged when
    // the payload names a privileged role, the second when the line assigns it to
    // a credential name. An anon or authenticated payload is neither.
    Pattern {
        provider: SUPABASE_SERVICE_ROLE,
        regex: r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
    },
    Pattern { provider: JWT, regex: r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b" },
    // Product, analytics and observability.
    Pattern { provider: "Airtable personal access token", regex: r"\bpat[0-9A-Za-z]{14}\.[0-9a-f]{60,}\b" },
    Pattern { provider: "Notion integration token", regex: r"\b(?:secret_[A-Za-z0-9]{43}|ntn_[A-Za-z0-9]{40,})\b" },
    Pattern { provider: "Linear API key", regex: r"\blin_api_[A-Za-z0-9]{40}\b" },
    Pattern { provider: "Asana personal access token", regex: r"\b[0-9]/[0-9]{16}:[0-9a-f]{32}\b" },
    Pattern { provider: "Atlassian API token", regex: r"\bATATT3[A-Za-z0-9_=-]{50,}\b" },
    Pattern {
        provider: "Jira API token",
        regex: assigned!(r#"(?:jira|confluence)[^"'\n]{0,20}token"#, r#"[A-Za-z0-9]{24,}"#),
    },
    Pattern { provider: "Sentry auth token", regex: r"\bsntrys_[A-Za-z0-9_=+/-]{40,}\b" },
    Pattern {
        provider: "Sentry DSN with a secret",
        regex: r"https://[0-9a-f]{32}:[0-9a-f]{32}@[A-Za-z0-9.-]*sentry\.io/[0-9]+",
    },
    Pattern { provider: "Datadog token", regex: r"\bdd[a-z]{2,}_[A-Za-z0-9]{30,}\b" },
    Pattern {
        provider: "Datadog API key",
        regex: assigned!(r#"datadog[^"'\n]{0,20}(?:api|app)[_-]?key"#, r#"[0-9a-f]{32,40}"#),
    },
    Pattern { provider: "New Relic user key", regex: r"\bNRAK-[A-Z0-9]{27}\b" },
    Pattern { provider: "New Relic license key", regex: r"\b[a-f0-9]{36}NRAL\b" },
    Pattern { provider: "Grafana token", regex: r"\bgl(?:c|sa)_[A-Za-z0-9_=+/-]{32,}\b" },
    Pattern {
        provider: "PagerDuty API key",
        regex: assigned!(r#"pagerduty[^"'\n]{0,25}(?:api[_-]?)?(?:key|token)"#, r#"[A-Za-z0-9_+-]{20,}"#),
    },
    Pattern {
        provider: "Opsgenie API key",
        regex: assigned!(
            r#"opsgenie[^"'\n]{0,25}(?:api[_-]?)?key"#,
            r#"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"#
        ),
    },
    Pattern { provider: "Rollbar access token", regex: assigned!(r#"rollbar[^"'\n]{0,25}token"#, r#"[0-9a-f]{32}"#) },
    Pattern {
        provider: "Bugsnag API key",
        regex: assigned!(r#"bugsnag[^"'\n]{0,25}(?:api[_-]?)?key"#, r#"[0-9a-f]{32}"#),
    },
    Pattern {
        provider: "Mezmo ingestion key",
        regex: assigned!(r#"(?:logdna|mezmo)[^"'\n]{0,25}key"#, r#"[0-9a-f]{32}"#),
    },
    Pattern {
        provider: "Segment write key",
        regex: assigned!(r#"segment[^"'\n]{0,25}write[_-]?key"#, r#"[A-Za-z0-9]{32}"#),
    },
    Pattern {
        provider: "Amplitude secret key",
        regex: assigned!(r#"amplitude[^"'\n]{0,25}secret[_-]?key"#, r#"[0-9a-f]{32}"#),
    },
    Pattern { provider: "Mixpanel API secret", regex: assigned!(r#"mixpanel[^"'\n]{0,25}secret"#, r#"[0-9a-f]{32}"#) },
    Pattern {
        provider: "Algolia admin key",
        regex: assigned!(r#"algolia[^"'\n]{0,25}(?:admin|write)[^"'\n]{0,12}key"#, r#"[0-9a-f]{32}"#),
    },
    Pattern { provider: "Mapbox secret token", regex: r"\bsk\.eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\b" },
    Pattern {
        provider: "LaunchDarkly SDK key",
        regex: r"\bsdk-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
    },
    Pattern { provider: "Statsig secret key", regex: r"\bsecret-[A-Za-z0-9]{40,}\b" },
    // Branch's `key_live_` is the Branch Key: the public half, in every mobile
    // bundle the SDK ships in. The secret half is `secret_live_`, and it is the
    // only one of the two worth a locked finding.
    Pattern { provider: "Branch.io secret key", regex: r"\bsecret_live_[A-Za-z0-9]{32}\b" },
    Pattern {
        provider: "OneSignal REST API key",
        regex: assigned!(r#"onesignal[^"'\n]{0,30}(?:rest[_-]?)?(?:api[_-]?)?key"#, r#"[A-Za-z0-9_-]{40,}"#),
    },
    Pattern {
        provider: "AppsFlyer dev key",
        regex: assigned!(r#"appsflyer[^"'\n]{0,25}dev[_-]?key"#, r#"[A-Za-z0-9]{20,}"#),
    },
    // `appl_` and `goog_` are RevenueCat's public SDK keys, which every client is
    // configured with and which are meant to be in the bundle. Only the `sk_`
    // key reaches the REST API, so the credential name alone is not enough.
    Pattern {
        provider: "RevenueCat secret key",
        regex: assigned!(r#"revenuecat[^"'\n]{0,25}(?:secret|api)[_-]?key"#, r#"sk_[A-Za-z0-9_-]{20,}"#),
    },
    Pattern {
        provider: "Elastic Cloud API key",
        regex: assigned!(r#"elastic[^"'\n]{0,25}(?:api[_-]?)?key"#, r#"[A-Za-z0-9=+/]{40,}"#),
    },
    // Commerce and payments.
    Pattern { provider: "Stripe secret key", regex: r"\b(?:sk|rk)_live_[0-9A-Za-z]{24,}\b" },
    Pattern { provider: "Stripe webhook signing secret", regex: r"\bwhsec_[0-9A-Za-z]{32,}\b" },
    Pattern { provider: "Shopify access token", regex: r"\bshp(?:at|ss|ca|pa)_[0-9a-fA-F]{32}\b" },
    Pattern { provider: "Square access token", regex: r"\bsq0atp-[0-9A-Za-z_-]{22}\b" },
    Pattern { provider: "Square OAuth secret", regex: r"\bsq0csp-[0-9A-Za-z_-]{43}\b" },
    Pattern {
        provider: "PayPal client secret",
        regex: assigned!(r#"paypal[^"'\n]{0,30}(?:client[_-]?)?secret"#, r#"E[A-Za-z0-9_-]{50,}"#),
    },
    Pattern {
        provider: "Braintree private key",
        regex: assigned!(r#"braintree[^"'\n]{0,25}private[_-]?key"#, r#"[0-9a-f]{32}"#),
    },
    Pattern { provider: "Adyen API key", regex: r"\bAQE[A-Za-z0-9+/=]{60,}\b" },
    Pattern {
        provider: "Plaid production access token",
        regex: r"\baccess-production-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
    },
    Pattern { provider: "Plaid secret", regex: assigned!(r#"plaid[^"'\n]{0,20}secret"#, r#"[0-9a-f]{30}"#) },
    Pattern {
        provider: "Coinbase API secret",
        regex: assigned!(r#"coinbase[^"'\n]{0,25}(?:api[_-]?)?secret"#, r#"[A-Za-z0-9+/=]{40,}"#),
    },
    Pattern {
        provider: "Kraken private key",
        regex: assigned!(r#"kraken[^"'\n]{0,25}(?:private|api)[_-]?(?:key|secret)"#, r#"[A-Za-z0-9+/=]{50,}"#),
    },
    Pattern { provider: "Binance secret key", regex: assigned!(r#"binance[^"'\n]{0,25}secret"#, r#"[A-Za-z0-9]{64}"#) },
    // Identity.
    Pattern { provider: "Okta API token", regex: r"\b00[A-Za-z0-9_-]{40}\b" },
    Pattern {
        provider: "Auth0 client secret",
        regex: assigned!(r#"auth0[^"'\n]{0,30}(?:client[_-]?)?secret"#, r#"[A-Za-z0-9_-]{32,}"#),
    },
    Pattern {
        provider: "Salesforce client secret",
        regex: assigned!(r#"salesforce[^"'\n]{0,30}(?:client[_-]?)?secret"#, r#"[0-9A-Za-z.]{40,}"#),
    },
    Pattern {
        provider: "Zoom API secret",
        regex: assigned!(r#"zoom[^"'\n]{0,25}(?:api[_-]?)?secret"#, r#"[A-Za-z0-9]{32,}"#),
    },
    // Content and storage.
    Pattern { provider: "Dropbox access token", regex: r"\bsl\.[A-Za-z0-9_-]{100,}\b" },
    Pattern { provider: "Figma personal access token", regex: r"\bfigd_[A-Za-z0-9_-]{40,}\b" },
    Pattern { provider: "Contentful management token", regex: r"\bCFPAT-[A-Za-z0-9_-]{43}\b" },
    Pattern { provider: "Sanity API token", regex: assigned!(r#"sanity[^"'\n]{0,25}token"#, r#"[A-Za-z0-9]{40,}"#) },
    Pattern {
        provider: "Storyblok management token",
        regex: assigned!(r#"storyblok[^"'\n]{0,30}token"#, r#"[A-Za-z0-9]{20,}"#),
    },
    Pattern { provider: "Cloudinary URL", regex: r"cloudinary://[0-9]{10,}:[A-Za-z0-9_-]{20,}@[a-z0-9-]+" },
    Pattern { provider: "Pusher app secret", regex: assigned!(r#"pusher[^"'\n]{0,25}secret"#, r#"[0-9a-f]{20,}"#) },
    Pattern {
        provider: "Stream API secret",
        regex: assigned!(r#"stream[^"'\n]{0,25}(?:api[_-]?)?secret"#, r#"[A-Za-z0-9]{40,}"#),
    },
    Pattern {
        provider: "HubSpot private app token",
        regex: r"\bpat-(?:na|eu)[0-9]?-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b",
    },
    Pattern {
        provider: "Trello API secret",
        regex: assigned!(r#"trello[^"'\n]{0,25}(?:api[_-]?)?(?:secret|token)"#, r#"[0-9a-f]{64}"#),
    },
    // Model providers.
    Pattern {
        provider: "OpenAI API key",
        regex: r"\b(?:sk-(?:proj|svcacct|admin)-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,})\b",
    },
    Pattern { provider: "Anthropic API key", regex: r"\bsk-ant-[a-z0-9]{3,}[0-9]{2}-[A-Za-z0-9_-]{20,}\b" },
    Pattern { provider: "Hugging Face token", regex: r"\bhf_[A-Za-z0-9]{34,}\b" },
    Pattern { provider: "Replicate API token", regex: r"\br8_[A-Za-z0-9]{37,}\b" },
    Pattern { provider: "Groq API key", regex: r"\bgsk_[A-Za-z0-9]{40,}\b" },
    Pattern { provider: "Perplexity API key", regex: r"\bpplx-[A-Za-z0-9]{32,}\b" },
    Pattern {
        provider: "Cohere API key",
        regex: assigned!(r#"cohere[^"'\n]{0,25}(?:api[_-]?)?key"#, r#"[A-Za-z0-9]{40}"#),
    },
    Pattern {
        provider: "Mistral API key",
        regex: assigned!(r#"mistral[^"'\n]{0,25}(?:api[_-]?)?key"#, r#"[A-Za-z0-9]{32}"#),
    },
    // Key material and headers.
    Pattern { provider: PRIVATE_KEY, regex: r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----" },
    Pattern { provider: BASIC_AUTH, regex: r#"(?i)basic\s+(?P<v>[A-Za-z0-9+/]{20,}={0,2})"# },
    Pattern { provider: BEARER, regex: assigned!(r#"authorization"#, r#"bearer\s+"#, r#"[A-Za-z0-9._-]{20,}"#) },
    // The three generic entries, every one of them behind the entropy gate.
    Pattern { provider: "password assignment", regex: assigned!(r#"(?:password|passwd|pwd)"#, r#"[^"'`]{8,}"#) },
    Pattern { provider: "API key assignment", regex: assigned!(r#"\b[a-z0-9_]*api[_-]?key"#, r#"[A-Za-z0-9_-]{20,}"#) },
    Pattern {
        provider: "secret assignment",
        regex: assigned!(r#"\b[a-z0-9_]*(?:secret|access[_-]?token|auth[_-]?token)"#, r#"[A-Za-z0-9_-]{20,}"#),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The table is the rule. A pattern that does not compile, a provider label
    /// used twice (the two JWT entries excepted, which are deliberately one
    /// shape under two gates) or a table that has shrunk below the hundred the
    /// plan asks for is all caught here rather than in a rule test.
    #[test]
    fn the_table_compiles_and_has_at_least_a_hundred_distinct_providers() {
        let c = compiled();
        assert_eq!(c.regexes.len(), PATTERNS.len());
        assert!(PATTERNS.len() >= 100, "{} patterns", PATTERNS.len());
        let mut seen = std::collections::BTreeSet::new();
        for p in PATTERNS {
            assert!(seen.insert(p.provider), "duplicate provider {}", p.provider);
        }
    }

    /// The generic entries sit last.
    ///
    /// [`super::scan`] reads a line's matches in table order and lets a generic
    /// entry step aside for a named one that has already claimed the value, so
    /// one committed key is one finding rather than two. A generic entry moved
    /// up the table would win that race and report `apiKey = "AIza..."` as an
    /// API key assignment instead of a Google API key.
    #[test]
    fn the_generic_entries_are_last_in_the_table() {
        let first = PATTERNS.iter().position(|p| GENERIC.contains(&p.provider)).expect("the generics are in the table");
        let after: Vec<&str> = PATTERNS[first..].iter().map(|p| p.provider).filter(|p| !GENERIC.contains(p)).collect();
        assert!(after.is_empty(), "a named entry sits after a generic one: {after:?}");
    }

    /// Every entry that names a value carries [`DELIMITER`] in front of it.
    ///
    /// This is the guard for the review finding that opened this pass: a context
    /// entry whose separator was a character class rather than a sequence read
    /// `const expoToken = getExpoTokenFromSecureStore()` as a credential, and
    /// forty five entries were spelled that way. A new provider added by hand
    /// rather than through [`assigned!`] fails here with its own label.
    #[test]
    fn every_context_entry_requires_a_delimited_literal() {
        for p in PATTERNS {
            if !p.regex.contains("(?P<v>") || NOT_ASSIGNMENTS.contains(&p.provider) {
                continue;
            }
            assert!(p.regex.contains(DELIMITER), "{} spells its own separator: {}", p.provider, p.regex);
        }
        // The class that let an identifier through. Nothing may reintroduce it.
        for p in PATTERNS {
            assert!(!p.regex.contains(r#"["'\s:=]+"#), "{} uses the loose separator", p.provider);
        }
    }
}
