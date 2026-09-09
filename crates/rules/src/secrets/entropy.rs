//! The gate the three generic patterns stand behind.
//!
//! `password = "..."` and `apiKey = "..."` are shapes a repository is full of:
//! a test double, a fixture, a translation string, a CSS class list, an enum
//! member. What separates a credential from all of them is that a credential
//! was generated rather than written, and generated strings look different:
//! long, drawn from several character classes, and near enough uniform that no
//! character carries much less surprise than any other. That is what
//! [`looks_random`] measures, and it is deliberately strict. Under-reporting a
//! generic assignment costs a finding the provider patterns mostly catch
//! anyway; over-reporting one costs the founder a blocked build on a rule that
//! cannot be turned off.

use std::collections::HashMap;

/// Shannon entropy of a string in bits per character. An empty string is zero.
pub fn shannon(s: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    let mut len = 0.0f64;
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
        len += 1.0;
    }
    if len == 0.0 {
        return 0.0;
    }
    -counts
        .values()
        .map(|&n| {
            let p = n as f64 / len;
            p * p.log2()
        })
        .sum::<f64>()
}

/// Whether a string looks generated rather than written. Every clause is a
/// separate way of being written by a person:
///
/// - **Twenty characters.** Shorter than that and the entropy figure is noise:
///   an eight character string of eight distinct characters scores 3.0 whatever
///   it says.
/// - **Three of the four character classes.** A generated credential mixes
///   cases and digits. A sentence, an identifier, a file path and a lowercase
///   hex digest do not.
/// - **Three and a half bits per character.** Uniform over twenty distinct
///   characters is 4.32; `Password1234!` shaped strings sit well under.
/// - **No whitespace.** No provider generates a credential with a space in it.
///   A string that has one is a sentence, and a repository is full of them:
///   `oauth2Password: "OAuth2 password grant"` is a label on a form, and it
///   scores over four bits per character.
/// - **Not a URL and not a path.** Both are long, mixed case and full of
///   punctuation, and neither is a secret on its own.
/// - **Not one group repeated.** `abcabcabcabcabcabcabc` passes every count
///   above and was obviously typed.
pub fn looks_random(s: &str) -> bool {
    let len = s.chars().count();
    if len < 20 {
        return false;
    }
    if s.chars().any(char::is_whitespace) || is_url(s) || is_path(s) {
        return false;
    }
    if repeats_one_group(s) {
        return false;
    }
    let lower = s.chars().any(|c| c.is_ascii_lowercase());
    let upper = s.chars().any(|c| c.is_ascii_uppercase());
    let digit = s.chars().any(|c| c.is_ascii_digit());
    let symbol = s.chars().any(|c| !c.is_ascii_alphanumeric());
    let classes = [lower, upper, digit, symbol].iter().filter(|c| **c).count();
    if classes < 3 {
        return false;
    }
    shannon(s) >= 3.5
}

fn is_url(s: &str) -> bool {
    s.contains("://") || s.starts_with("www.")
}

/// A path is a string with a separator in it whose characters are all ones a
/// path is made of. A base64 credential also carries `/`, but it carries `+`
/// or `=` or a mixed case run with it, which no path segment does.
fn is_path(s: &str) -> bool {
    (s.contains('/') || s.contains('\\')) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/\\@ ".contains(c))
}

fn repeats_one_group(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    for period in 1..=n / 2 {
        if !n.is_multiple_of(period) {
            continue;
        }
        if (period..n).all(|i| chars[i] == chars[i % period]) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entropy_counts_surprise_per_character() {
        assert_eq!(shannon(""), 0.0);
        assert_eq!(shannon("aaaa"), 0.0);
        assert_eq!(shannon("ab"), 1.0);
        assert_eq!(shannon("abcd"), 2.0);
    }

    #[test]
    fn generated_strings_pass_and_written_ones_do_not() {
        assert!(looks_random("Xq7Rt2Lm9Pv4Zn8Kb3Wd6Yc1"), "a generated credential");
        assert!(!looks_random("hunter2"), "too short");
        assert!(!looks_random("correct horse battery staple"), "one class and a space");
        // The corpus false positive this clause was added for: a form label,
        // four character classes, and over four bits per character.
        assert!(!looks_random("OAuth2 password grant"), "prose with spaces");
        assert!(!looks_random("abcabcabcabcabcabcabcabc"), "one group repeated");
        assert!(!looks_random("aaaaaaaaaaaaaaaaaaaaaaaa"), "one character");
        assert!(!looks_random("src/components/Button.tsx"), "a path");
        assert!(!looks_random("https://api.example.com/v1/users"), "a URL");
        assert!(!looks_random("0123456789abcdef0123456789abcdef"), "two classes only");
        assert!(!looks_random("PasswordPasswordPassword"), "one group repeated, mixed case");
    }
}
