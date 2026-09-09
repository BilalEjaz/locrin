//! SARIF 2.1.0 for GitHub code scanning and third-party tools (spec 7.3). The
//! full verdict, never the agent cap: a CI consumer wants everything.

use locrin_core::finding::{Category, Finding, Severity, Verdict};
use serde_json::{json, Value};

/// One rule as the driver advertises it. The catalogue is the whole rule set,
/// including a rule that ships off, so a consumer reading the log sees every
/// rule the tool knows and `enabledByDefault` says which ones ran unasked.
pub struct RuleMeta {
    pub id: String,
    pub description: String,
    pub severity: Severity,
    pub category: Category,
    pub enabled_by_default: bool,
}

fn level(s: Severity) -> &'static str {
    match s {
        Severity::High => "error",
        Severity::Medium => "warning",
        Severity::Low => "note",
    }
}

fn result(f: &Finding, rule_index: Option<usize>) -> Value {
    let mut r = json!({
        "ruleId": f.rule,
        "level": level(f.severity),
        "message": { "text": format!("{} Fix: {}", f.evidence, f.fix) },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": f.file, "uriBaseId": "%SRCROOT%" },
                "region": {
                    "startLine": f.span.start_line,
                    "startColumn": f.span.start_col + 1,
                    "endLine": f.span.end_line,
                    "endColumn": f.span.end_col + 1
                }
            }
        }],
        "partialFingerprints": { "locrin/id": f.id },
        "properties": { "confidence": f.confidence, "category": f.category, "related": f.related }
    });
    if let Some(i) = rule_index {
        r["ruleIndex"] = json!(i);
    }
    if let Some(o) = &f.owasp {
        r["properties"]["owasp"] = json!(o);
    }
    if let Some(c) = &f.cwe {
        r["properties"]["cwe"] = json!(c);
    }
    r
}

pub fn render(v: &Verdict, rules: &[RuleMeta], version: &str) -> String {
    let results: Vec<Value> = v.findings.iter().map(|f| result(f, rules.iter().position(|r| r.id == f.rule))).collect();
    let rule_objects: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.id,
                "shortDescription": { "text": r.description },
                // `enabled` is SARIF's own `reportingConfiguration` field, so a
                // consumer that reads the standard (GitHub code scanning does)
                // knows a rule ships off without knowing locrin's properties.
                // The property stays beside it for consumers already reading it.
                "defaultConfiguration": { "level": level(r.severity), "enabled": r.enabled_by_default },
                "properties": { "category": r.category, "enabledByDefault": r.enabled_by_default }
            })
        })
        .collect();
    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "locrin",
                "version": version,
                "informationUri": "https://locrin.com",
                "rules": rule_objects
            } },
            "invocations": [{ "executionSuccessful": true, "exitCode": v.exit_code() }],
            "results": results
        }]
    });
    serde_json::to_string_pretty(&doc).expect("sarif serialises")
}

#[cfg(test)]
mod tests {
    use super::*;
    use locrin_core::finding::*;

    fn f(rule: &str, sev: Severity, line: u32) -> Finding {
        Finding {
            id: format!("{:016x}", line),
            rule: rule.into(),
            category: Category::Erosion,
            severity: sev,
            confidence: Confidence::High,
            file: "src/a.ts".into(),
            span: Span { start_line: line, start_col: 2, end_line: line, end_col: 9 },
            evidence: "console.log(1)".into(),
            fix: "Remove it".into(),
            related: vec!["src/b.ts".into()],
            owasp: None,
            cwe: Some("CWE-1".into()),
        }
    }

    fn rules() -> Vec<RuleMeta> {
        vec![
            RuleMeta {
                id: "leftover-debug".into(),
                description: "d1".into(),
                severity: Severity::High,
                category: Category::Erosion,
                enabled_by_default: true,
            },
            RuleMeta {
                id: "dead-export".into(),
                description: "d2".into(),
                severity: Severity::Low,
                category: Category::Erosion,
                enabled_by_default: false,
            },
        ]
    }

    #[test]
    fn renders_a_full_uncapped_sarif_log() {
        let many: Vec<Finding> = (0..15).map(|i| f("leftover-debug", Severity::High, i + 1)).collect();
        let v = Verdict::from_findings(many, 3);
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        assert_eq!(doc["version"], "2.1.0");
        assert_eq!(doc["$schema"], "https://json.schemastore.org/sarif-2.1.0.json");
        let run = &doc["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "locrin");
        assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 2);
        assert_eq!(run["results"].as_array().unwrap().len(), 15, "never capped");
        assert_eq!(run["invocations"][0]["exitCode"], 1);
    }

    #[test]
    fn the_catalogue_carries_every_rule_with_its_default_enablement() {
        let v = Verdict::from_findings(vec![], 1);
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        let listed = doc["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(listed.len(), 2, "a rule that ships off is still advertised");
        assert_eq!(listed[0]["properties"]["enabledByDefault"], true);
        assert_eq!(listed[1]["id"], "dead-export");
        assert_eq!(listed[1]["properties"]["enabledByDefault"], false);
        assert_eq!(listed[1]["defaultConfiguration"]["level"], "note");
        assert_eq!(listed[0]["defaultConfiguration"]["enabled"], true);
        assert_eq!(listed[1]["defaultConfiguration"]["enabled"], false, "SARIF's own field says the rule ships off");
    }

    #[test]
    fn maps_levels_columns_fingerprints_and_properties() {
        let v = Verdict::from_findings(
            vec![f("dead-export", Severity::Low, 4), f("leftover-debug", Severity::Medium, 2)],
            1,
        );
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        let results = doc["runs"][0]["results"].as_array().unwrap();
        let by_rule = |id: &str| results.iter().find(|r| r["ruleId"] == id).unwrap().clone();
        let low = by_rule("dead-export");
        assert_eq!(low["level"], "note");
        assert_eq!(low["ruleIndex"], 1);
        let region = &low["locations"][0]["physicalLocation"]["region"];
        assert_eq!(
            (region["startLine"].as_u64(), region["startColumn"].as_u64(), region["endColumn"].as_u64()),
            (Some(4), Some(3), Some(10))
        );
        assert_eq!(low["locations"][0]["physicalLocation"]["artifactLocation"]["uriBaseId"], "%SRCROOT%");
        assert_eq!(low["partialFingerprints"]["locrin/id"], "0000000000000004");
        assert_eq!(low["properties"]["cwe"], "CWE-1");
        assert_eq!(low["properties"]["related"][0], "src/b.ts");
        assert!(low["properties"].get("owasp").is_none());
        assert_eq!(
            by_rule("leftover-debug")["level"],
            "warning",
            "level follows the finding's severity, not the rule default"
        );
    }

    /// `dead-file` and `boundary-violation` point at a whole file rather than at
    /// a run of characters, so they carry a zero-width span. Shifted to SARIF's
    /// 1-based columns that is `startColumn == endColumn == 1`, a legal
    /// insertion point at the head of the line, which is exactly what a
    /// file-level finding means. Pinned here because the shift is the only thing
    /// keeping the column out of SARIF's illegal column 0.
    #[test]
    fn a_zero_width_span_becomes_a_legal_insertion_point() {
        let mut finding = f("dead-file", Severity::Medium, 1);
        finding.span = Span { start_line: 1, start_col: 0, end_line: 1, end_col: 0 };
        let v = Verdict::from_findings(vec![finding], 1);
        let doc: Value = serde_json::from_str(&render(&v, &rules(), "0.1.0")).unwrap();
        let region = &doc["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startColumn"], 1, "{region}");
        assert_eq!(region["endColumn"], 1, "{region}");
    }
}
