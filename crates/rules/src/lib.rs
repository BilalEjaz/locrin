use locrin_core::config::Config;
use locrin_core::finding::{make_id, Category, Confidence, Finding, Severity, Span};
use locrin_core::parse::ParsedFile;
use locrin_core::symbols::enclosing_symbol;

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

/// Runs the given rules, skipping any the config disables and applying the
/// configured severity to every finding a rule produces.
pub fn run_rules(rules: &[Box<dyn Rule>], ctx: &RuleContext) -> Vec<Finding> {
    let mut out = Vec::new();
    for rule in rules {
        if !ctx.config.rule_enabled(rule.id()) {
            continue;
        }
        let severity = ctx.config.severity_for(rule.id(), rule.default_severity());
        for mut f in rule.run(ctx) {
            f.severity = severity;
            out.push(f);
        }
    }
    out
}

/// The registry. Empty until the first rule is registered.
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![]
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
    fn registry_is_empty_until_a_rule_is_added() {
        let files: Vec<locrin_core::parse::ParsedFile> = vec![];
        let config = Config::default();
        let ctx = RuleContext { files: &files, config: &config };
        assert!(all_rules().is_empty());
        assert!(run_all(&ctx).is_empty());
    }
}
