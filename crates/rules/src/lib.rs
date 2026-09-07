pub mod leftover_commented;
pub mod leftover_debug;
pub mod leftover_marker;

use std::collections::HashMap;

use locrin_core::config::Config;
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use locrin_core::parse::ParsedFile;
use locrin_core::symbols::enclosing_symbol;

/// The in-source suppression marker. Honoured for every rule: a finding whose
/// line carries this text is dropped in `run_rules`, so rules never have to
/// implement suppression themselves.
pub const ALLOW_MARK: &str = "locrin:allow";

/// Everything a rule is allowed to see: the parsed files of this run and the
/// repository config. Rules never touch the filesystem themselves.
pub struct RuleContext<'a> {
    pub files: &'a [ParsedFile],
    pub config: &'a Config,
}

pub trait Rule {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn default_severity(&self) -> Severity;
    fn confidence(&self) -> Confidence;
    fn run(&self, ctx: &RuleContext) -> Vec<Finding>;
}

/// The files a rule is allowed to look at: every parsed file whose tree came
/// back without a syntax error. This is the single spec 9 gate. Rules iterate
/// this instead of `ctx.files`, so a file that failed to parse is exempt from
/// every rule rather than from whichever rules happened to check `has_error`.
pub fn clean_files<'a>(ctx: &'a RuleContext) -> impl Iterator<Item = &'a ParsedFile> {
    let files: &'a [ParsedFile] = ctx.files;
    files.iter().filter(|f| !f.has_error)
}

/// The trimmed text of a 1-based line, or `""` when the line is out of range.
pub fn line_text(file: &ParsedFile, line: u32) -> &str {
    file.source.lines().nth(line.saturating_sub(1) as usize).unwrap_or("").trim()
}

/// The identity anchor for a finding: the enclosing symbol name if there is
/// one, otherwise the line text. Anchoring on the symbol is what keeps a
/// finding's id stable when unrelated lines move around it.
pub fn anchor_for(file: &ParsedFile, line: u32) -> String {
    enclosing_symbol(file, line).unwrap_or_else(|| line_text(file, line).to_string())
}

/// Builds a finding for a rule at a line, filling in the identity, span, and
/// the rule's own category, severity, and confidence. Public because every rule
/// (and the trait's own tests) construct findings through it rather than by
/// hand, which is what keeps ids consistent across rules.
pub fn finding(rule: &dyn Rule, file: &ParsedFile, line: u32, evidence: &str, fix: &str) -> Finding {
    let text = line_text(file, line);
    Finding {
        id: make_id(rule.id(), &file.rel, &anchor_for(file, line)),
        rule: rule.id().to_string(),
        category: rule.category(),
        severity: rule.default_severity(),
        confidence: rule.confidence(),
        file: file.rel.clone(),
        span: Span { start_line: line, start_col: 0, end_line: line, end_col: text.len() as u32 },
        evidence: evidence.to_string(),
        fix: fix.to_string(),
        related: vec![],
        owasp: None,
        cwe: None,
    }
}

/// Runs the given rules, skipping any the config disables, dropping any finding
/// that came from a file which failed to parse or whose line carries the
/// `locrin:allow` marker, and applying the configured severity to every finding
/// that survives.
///
/// The spec 9 gate is enforced here rather than left to the rules. `clean_files`
/// stays the way a rule should iterate, because skipping an unparsable file is
/// cheaper than walking it, but a rule that forgets to use it still cannot emit
/// a finding against a file the engine could not parse.
pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> Vec<Finding> {
    let by_rel: HashMap<&str, &ParsedFile> = ctx.files.iter().map(|f| (f.rel.as_str(), f)).collect();
    let unparsable = |f: &Finding| by_rel.get(f.file.as_str()).is_some_and(|file| file.has_error);
    let allowed = |f: &Finding| {
        by_rel.get(f.file.as_str()).is_some_and(|file| line_text(file, f.span.start_line).contains(ALLOW_MARK))
    };
    let mut out = Vec::new();
    for rule in rules {
        if !ctx.config.rule_enabled(rule.id()) {
            continue;
        }
        let severity = ctx.config.severity_for(rule.id(), rule.default_severity());
        for mut f in rule.run(ctx) {
            if unparsable(&f) || allowed(&f) {
                continue;
            }
            f.severity = severity;
            out.push(f);
        }
    }
    out
}

/// The registry: every rule the engine ships, in the order they are declared.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(leftover_debug::LeftoverDebug),
        Box::new(leftover_commented::LeftoverCommented),
        Box::new(leftover_marker::LeftoverMarker),
    ]
}

pub fn run_all(ctx: &RuleContext) -> Vec<Finding> {
    run_rules(&all_rules(), ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::config::Config;
    use locrin_core::finding::{Category, Confidence, Severity};
    use locrin_core::parse::parse_source;
    use std::path::Path;

    struct Always;
    impl Rule for Always {
        fn id(&self) -> &'static str {
            "always"
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
            clean_files(ctx).map(|f| finding(self, f, 1, "hit", "remove it")).collect()
        }
    }

    /// A rule that ignores `clean_files` and walks `ctx.files` directly, the way
    /// a careless third-party rule would. `run_rules` has to hold the spec 9
    /// gate on its own, not trust the rule to hold it.
    struct Careless;
    impl Rule for Careless {
        fn id(&self) -> &'static str {
            "careless"
        }
        fn category(&self) -> Category {
            Category::Erosion
        }
        fn default_severity(&self) -> Severity {
            Severity::Medium
        }
        fn confidence(&self) -> Confidence {
            Confidence::Medium
        }
        fn run(&self, ctx: &RuleContext) -> Vec<Finding> {
            ctx.files.iter().map(|f| finding(self, f, 1, "hit", "remove it")).collect()
        }
    }

    #[test]
    fn run_rules_applies_config_overrides() {
        let file = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let files = vec![file];
        let mut config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        let out = run_rules(&[Box::new(Always)], &ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].severity, Severity::Medium);
        assert_eq!(out[0].id.len(), 16);

        config
            .rules
            .insert("always".into(), locrin_core::config::RuleOverride { enabled: None, severity: Some(Severity::Low) });
        let ctx = RuleContext { files: &files, config: &config };
        assert_eq!(run_rules(&[Box::new(Always)], &ctx)[0].severity, Severity::Low);

        config
            .rules
            .insert("always".into(), locrin_core::config::RuleOverride { enabled: Some(false), severity: None });
        let ctx = RuleContext { files: &files, config: &config };
        assert!(run_rules(&[Box::new(Always)], &ctx).is_empty());
    }

    #[test]
    fn anchor_prefers_enclosing_symbol() {
        let file = parse_source(
            Path::new("src/a.ts"),
            "src/a.ts",
            "function f() {\n  console.log(1);\n}\nconsole.log(2);\n".into(),
        )
        .unwrap();
        assert_eq!(anchor_for(&file, 2), "f");
        assert_eq!(anchor_for(&file, 4), "console.log(2);");
        assert_eq!(line_text(&file, 4), "console.log(2);");
    }

    #[test]
    fn a_file_with_a_parse_error_is_exempt_from_every_rule() {
        let clean = parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1;\n".into()).unwrap();
        let broken =
            parse_source(Path::new("src/b.ts"), "src/b.ts", "export function broken( { return 1;\n".into()).unwrap();
        assert!(broken.has_error, "the fixture source must not parse cleanly");
        let files = vec![clean, broken];
        let config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        let out = run_rules(&[Box::new(Careless)], &ctx);
        assert_eq!(out.len(), 1, "only the clean file should survive run_rules, got {out:?}");
        assert_eq!(out[0].file, "src/a.ts");
    }

    #[test]
    fn allow_marker_suppresses_a_finding_from_any_rule() {
        let file =
            parse_source(Path::new("src/a.ts"), "src/a.ts", "export const a = 1; // locrin:allow\n".into()).unwrap();
        let files = vec![file];
        let config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        assert!(run_rules(&[Box::new(Always)], &ctx).is_empty());
    }

    #[test]
    fn registry_lists_every_shipped_rule() {
        let files: Vec<locrin_core::parse::ParsedFile> = vec![];
        let config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        let ids: Vec<&str> = all_rules().iter().map(|r| r.id()).collect();
        assert_eq!(ids, vec!["leftover-debug", "leftover-commented-code", "leftover-agent-marker"]);
        assert!(run_all(&ctx).is_empty(), "no files means no findings");
    }
}
