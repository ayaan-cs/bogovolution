//! Short failure notes for the mutation prompt.
//!
//! After a task fails, the harness keeps a few hundred bytes that say what went
//! wrong: which files the model wrote and the end of what the check printed.
//! The mutating model reads these notes, so it can propose a rule that
//! addresses a real failure instead of guessing.
//!
//! Hidden files must not leak through the notes. A check's output can quote
//! its own source (a Python traceback prints the failing test line, pytest
//! prints whole test bodies), so every output line that matches a line of a
//! hidden file is replaced with a placeholder before anything is kept. What
//! remains is what the check reports about the failure: error names,
//! messages, and the values it compared.

use std::collections::HashSet;

/// Most bytes in one task's note, kept in the log.
pub const MAX_DETAIL_BYTES: usize = 600;
/// Most bytes of hidden file text read to build the redaction list.
pub const MAX_HIDDEN_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// Hidden lines at least this long are also matched inside output lines.
const LONG_LINE: usize = 24;
/// Hidden lines shorter than this are too common to matter ("}", "else:").
const SHORT_LINE: usize = 4;
pub const REDACTED: &str = "[line from a hidden file]";

/// Lines of the hidden files, for redaction.
#[derive(Debug, Clone, Default)]
pub struct HiddenLines {
    exact: HashSet<String>,
    long: Vec<String>,
    /// Every kept hidden line, one per line, to find output lines that quote
    /// only part of a hidden line (a docstring without its quotes, say).
    blob: String,
}

fn norm(line: &str) -> &str {
    line.trim()
        .trim_start_matches(['>', ' ', '\t', '+', '-'])
        .trim_matches(['"', '\'', '`', '#', ' ', '\t'])
        .trim()
}

impl HiddenLines {
    pub fn from_texts<I: IntoIterator<Item = String>>(texts: I) -> HiddenLines {
        let mut h = HiddenLines::default();
        for text in texts {
            for line in text.lines() {
                let n = norm(line);
                if n.chars().count() < SHORT_LINE {
                    continue;
                }
                if h.exact.insert(n.to_string()) {
                    h.blob.push_str(n);
                    h.blob.push('\n');
                    if n.chars().count() >= LONG_LINE {
                        h.long.push(n.to_string());
                    }
                }
            }
        }
        h
    }

    /// Does this output line quote a hidden file?
    pub fn matches(&self, line: &str) -> bool {
        let n = norm(line);
        if n.is_empty() {
            return false;
        }
        self.exact.contains(n)
            || self.long.iter().any(|h| n.contains(h.as_str()))
            || (n.chars().count() >= LONG_LINE && self.blob.contains(n))
    }
}

/// Remove terminal color codes.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\r' || (c.is_control() && c != '\n' && c != '\t') {
            continue;
        }
        out.push(c);
    }
    out
}

/// Replace the folder `root` with `.` where it appears as a whole path
/// (`/work` and `/work/x`, but not `/workshop`).
fn replace_path(text: &str, root: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(root) {
        let after = &rest[i + root.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')));
        out.push_str(&rest[..i]);
        out.push_str(if whole { "." } else { root });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Clean check output: no color codes, worktree paths shown as `.`, and every
/// line that quotes a hidden file replaced (runs of them become one line).
pub fn redact(output: &str, hidden: &HiddenLines, roots: &[&str]) -> String {
    let mut text = strip_ansi(output);
    for root in roots.iter().filter(|r| !r.is_empty()) {
        text = replace_path(&text, root);
    }
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if hidden.matches(line) {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let placeholder = format!("{indent}{REDACTED}");
            if out.last() != Some(&placeholder) {
                out.push(placeholder);
            }
        } else {
            out.push(line.trim_end().to_string());
        }
    }
    // drop blank lines at both ends
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    let start = out.iter().position(|l| !l.is_empty()).unwrap_or(out.len());
    out[start..].join("\n")
}

/// The last `max` bytes of `text`, cut at a line start, with `...` in front
/// when something was dropped.
pub fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let budget = max.saturating_sub(4);
    let mut start = text.len() - budget;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let mut rest = &text[start..];
    if let Some(i) = rest.find('\n') {
        if i + 1 < rest.len() {
            rest = &rest[i + 1..];
        }
    }
    format!("...\n{rest}")
}

/// The first `max` bytes of `text`, at a char boundary.
pub fn head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max.saturating_sub(3);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

fn written(files: &[String]) -> String {
    if files.is_empty() {
        return "Wrote no files.".to_string();
    }
    let shown: Vec<&str> = files.iter().take(5).map(String::as_str).collect();
    let more = files.len().saturating_sub(shown.len());
    let mut s = format!("Wrote: {}", shown.join(", "));
    if more > 0 {
        s.push_str(&format!(" (and {more} more)"));
    }
    head(&s, 200)
}

/// Note for a task whose check failed or timed out.
pub fn check_note(
    files: &[String],
    timed_out_after: Option<u64>,
    output: &str,
    hidden: &HiddenLines,
    roots: &[&str],
) -> String {
    let mut head_text = written(files);
    if let Some(secs) = timed_out_after {
        head_text.push_str(&format!(
            "\nThe check ran past its {secs} s limit and was stopped."
        ));
    }
    let clean = redact(output, hidden, roots);
    if clean.trim().is_empty() {
        return head(
            &format!("{head_text}\nThe check printed nothing."),
            MAX_DETAIL_BYTES,
        );
    }
    head_text.push_str("\nEnd of the check output:\n");
    let room = MAX_DETAIL_BYTES.saturating_sub(head_text.len());
    format!("{head_text}{}", tail(&clean, room))
}

/// Note for a model call that failed.
pub fn model_error_note(message: &str) -> String {
    head(
        &format!("The model call failed: {}", message.trim()),
        MAX_DETAIL_BYTES,
    )
}

/// Note for an edit to a path the model may not touch.
pub fn violation_note(message: &str) -> String {
    head(
        &format!(
            "The answer tried to write a path it may not touch, so nothing was applied: {}",
            message.trim()
        ),
        MAX_DETAIL_BYTES,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(texts: &[&str]) -> HiddenLines {
        HiddenLines::from_texts(texts.iter().map(|s| s.to_string()))
    }

    #[test]
    fn hidden_lines_are_redacted_and_the_failure_stays() {
        let h = hidden(&[
            "def test_take(self):\n    self.assertEqual(take(3, range(10)), [0, 1, 2])\n",
        ]);
        let out = "Traceback (most recent call last):\n  File \"/work/checks/test_take.py\", line 2, in test_take\n    self.assertEqual(take(3, range(10)), [0, 1, 2])\n  File \"/work/solve/take.py\", line 3, in take\n    return list(islice(iterable, n))\nNameError: name 'islice' is not defined\n";
        let r = redact(out, &h, &["/work"]);
        assert!(!r.contains("assertEqual"), "{r}");
        assert!(r.contains(REDACTED));
        assert!(r.contains("NameError: name 'islice' is not defined"));
        assert!(r.contains("./solve/take.py"));
        assert!(!r.contains("/work"));
    }

    #[test]
    fn pytest_style_and_substring_quotes_are_redacted() {
        let h = hidden(&["    assert parse_config('a=1') == {'a': 1}  # the secret expectation\n"]);
        let out = ">       assert parse_config('a=1') == {'a': 1}  # the secret expectation\nE       AssertionError: assert {} == {'a': 1}\nfoo: assert parse_config('a=1') == {'a': 1}  # the secret expectation, inline\n";
        let r = redact(out, &h, &[]);
        assert!(!r.contains("secret expectation"), "{r}");
        assert!(r.contains("AssertionError: assert {} == {'a': 1}"));
        assert_eq!(r.matches(REDACTED).count(), 2, "{r}");
        // a run of redacted lines collapses to one placeholder
        let h = hidden(&["first hidden line\nsecond hidden line\n"]);
        let r = redact("first hidden line\nsecond hidden line\nError", &h, &[]);
        assert_eq!(r, format!("{REDACTED}\nError"));
    }

    #[test]
    fn part_of_a_hidden_line_is_redacted() {
        // unittest prints the first line of a test docstring, without quotes
        let h = hidden(&["    def test_x(self):\n        \"\"\"Taking more than an iterator has remaining should return what the\n        iterator has.\"\"\"\n"]);
        let r = redact(
            "Taking more than an iterator has remaining should return what the\nNameError: name 'islice' is not defined\n",
            &h,
            &[],
        );
        assert_eq!(
            r,
            format!("{REDACTED}\nNameError: name 'islice' is not defined")
        );
    }

    #[test]
    fn short_common_lines_are_not_redacted() {
        let h = hidden(&["}\nok\nelse:\n"]);
        assert!(!h.matches("ok"));
        assert!(!h.matches("}"));
    }

    #[test]
    fn notes_are_bounded_and_say_what_was_written() {
        let h = HiddenLines::default();
        let long: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let n = check_note(&["a.py".into()], None, &long, &h, &[]);
        assert!(n.len() <= MAX_DETAIL_BYTES, "{}", n.len());
        assert!(n.starts_with("Wrote: a.py\n"));
        assert!(n.contains("line 1999"), "the end of the output is kept");
        assert!(!n.contains("line 0\n"));
        let n = check_note(&[], Some(60), "", &h, &[]);
        assert!(n.contains("Wrote no files."));
        assert!(n.contains("60 s limit"));
        assert!(n.contains("printed nothing"));
        let many: Vec<String> = (0..50).map(|i| format!("file{i}.txt")).collect();
        let n = check_note(&many, None, "x", &h, &[]);
        assert!(n.contains("(and 45 more)"));
        assert!(n.len() <= MAX_DETAIL_BYTES);
        let n = model_error_note(&"x".repeat(5000));
        assert!(n.len() <= MAX_DETAIL_BYTES);
        let n = violation_note("path ../x is outside");
        assert!(n.contains("../x"));
    }

    #[test]
    fn only_whole_paths_are_replaced() {
        let r = redact(
            "/work/a.py /workshop/b /work\n/tmp/wt-1/c",
            &HiddenLines::default(),
            &["/tmp/wt-1", "/work"],
        );
        assert_eq!(r, "./a.py /workshop/b .\n./c");
    }

    #[test]
    fn color_codes_and_carriage_returns_go() {
        let r = redact(
            "\u{1b}[31mFAIL\u{1b}[0m\r\nok\n",
            &HiddenLines::default(),
            &[],
        );
        assert_eq!(r, "FAIL\nok");
    }

    #[test]
    fn tail_and_head_respect_char_boundaries() {
        let s = "é".repeat(100);
        assert!(tail(&s, 51).len() <= 51);
        assert!(head(&s, 51).len() <= 51);
        assert_eq!(tail("short", 10), "short");
    }
}
