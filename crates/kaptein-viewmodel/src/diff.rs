//! Unified diff — "diff before apply" (M1.3).
//!
//! A dependency-free, renderer-agnostic unified diff over lines. The edit flow compares
//! the live object's YAML against the edited manifest and shows what would change; this
//! is the *semantic* "before/after" the frontend renders (additions, removals, context).
//! It is deliberately small — the full three-way merge lives in Phase 2 with GitOps.

/// A single diff line: its kind (`+` added, `-` removed, ` ` context) and the line text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    /// `+`, `-`, or ` ` (context).
    pub tag: char,
    pub text: String,
}

/// A complete unified diff, split into hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedDiff {
    /// The hunks (each a contiguous run of changes plus surrounding context).
    pub hunks: Vec<Vec<DiffLine>>,
    /// Total lines added.
    pub added: usize,
    /// Total lines removed.
    pub removed: usize,
}

/// Compute a unified diff between `old` and `new` (as whole strings). Uses a
/// longest-common-subsequence algorithm over lines; `context` is the number of unchanged
/// lines to keep around each change (0 = minimal diff).
pub fn unified_diff(old: &str, new: &str, context: usize) -> UnifiedDiff {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // LCS DP over lines.
    let (n, m) = (old_lines.len(), new_lines.len());
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    // Backtrack to produce the edit script.
    #[derive(PartialEq)]
    enum Op {
        Keep,
        Remove,
        Add,
    }
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old_lines[i] == new_lines[j] {
            ops.push(Op::Keep);
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(Op::Remove);
            i += 1;
        } else {
            ops.push(Op::Add);
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Remove);
        i += 1;
    }
    while j < m {
        ops.push(Op::Add);
        j += 1;
    }

    // Build tagged lines.
    let mut tagged: Vec<DiffLine> = Vec::new();
    let (mut oi, mut ni) = (0usize, 0usize);
    let (mut added, mut removed) = (0usize, 0usize);
    for op in &ops {
        match op {
            Op::Keep => {
                tagged.push(DiffLine {
                    tag: ' ',
                    text: old_lines[oi].to_string(),
                });
                oi += 1;
                ni += 1;
            }
            Op::Remove => {
                tagged.push(DiffLine {
                    tag: '-',
                    text: old_lines[oi].to_string(),
                });
                removed += 1;
                oi += 1;
            }
            Op::Add => {
                tagged.push(DiffLine {
                    tag: '+',
                    text: new_lines[ni].to_string(),
                });
                added += 1;
                ni += 1;
            }
        }
    }

    // Group into hunks with context.
    let hunks = group_hunks(&tagged, context);

    UnifiedDiff {
        hunks,
        added,
        removed,
    }
}

/// Render the diff as a string in unified-diff format (for the CLI).
pub fn render_unified(diff: &UnifiedDiff, old_label: &str, new_label: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("--- {old_label}\n+++ {new_label}\n"));
    for (i, hunk) in diff.hunks.iter().enumerate() {
        out.push_str(&format!("@@ hunk {} @@\n", i + 1));
        for line in hunk {
            out.push(line.tag);
            out.push(' ');
            out.push_str(&line.text);
            out.push('\n');
        }
    }
    out
}

fn group_hunks(tagged: &[DiffLine], context: usize) -> Vec<Vec<DiffLine>> {
    // Identify the indices of changed lines (`+` or `-`).
    let is_changed = |l: &DiffLine| l.tag == '+' || l.tag == '-';

    let mut hunks: Vec<Vec<DiffLine>> = Vec::new();
    let mut i = 0usize;
    let n = tagged.len();

    while i < n {
        if !is_changed(&tagged[i]) {
            i += 1;
            continue;
        }
        // Start of a change run: include `context` lines before.
        let start = i.saturating_sub(context);
        // Extend to include `context` lines after the last change in this run.
        let mut end = i;
        while end < n && is_changed(&tagged[end]) {
            end += 1;
        }
        // Include trailing context after the change run.
        let mut trailing = end;
        let mut count = 0;
        while trailing < n && count < context {
            if is_changed(&tagged[trailing]) {
                break;
            }
            trailing += 1;
            count += 1;
        }
        let hunk: Vec<DiffLine> = tagged[start..trailing].to_vec();
        if !hunk.is_empty() {
            hunks.push(hunk);
        }
        i = trailing.max(end).max(i + 1);
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_produce_no_changes() {
        let d = unified_diff("a\nb\nc", "a\nb\nc", 0);
        assert_eq!(d.added, 0);
        assert_eq!(d.removed, 0);
        assert!(d.hunks.is_empty());
    }

    #[test]
    fn single_line_change_is_detected() {
        let d = unified_diff("a\nb\nc", "a\nx\nc", 0);
        assert_eq!(d.added, 1);
        assert_eq!(d.removed, 1);
        // The hunk contains the removed 'b' and added 'x'.
        let hunk = &d.hunks[0];
        assert!(hunk.iter().any(|l| l.tag == '-' && l.text == "b"));
        assert!(hunk.iter().any(|l| l.tag == '+' && l.text == "x"));
    }

    #[test]
    fn context_lines_are_included() {
        let d = unified_diff("a\nb\nc\nd\ne", "a\nb\nX\nd\ne", 1);
        // With context 1, the hunk includes 'b' (before) and 'd' (after) as context.
        let hunk = &d.hunks[0];
        assert!(hunk.iter().any(|l| l.tag == ' ' && l.text == "b"));
        assert!(hunk.iter().any(|l| l.tag == ' ' && l.text == "d"));
        assert!(hunk.iter().any(|l| l.tag == '+' && l.text == "X"));
    }

    #[test]
    fn render_unified_has_header_and_hunks() {
        let d = unified_diff("a\nb", "a\nc", 0);
        let s = render_unified(&d, "old.yaml", "new.yaml");
        assert!(s.contains("--- old.yaml"));
        assert!(s.contains("+++ new.yaml"));
        assert!(s.contains("- b"));
        assert!(s.contains("+ c"));
    }
}
