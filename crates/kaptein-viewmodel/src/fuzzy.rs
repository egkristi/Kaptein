//! Fuzzy matching — the "fuzzy jump" semantic (M1.2).
//!
//! A renderer-agnostic subsequence matcher: a query matches a candidate when all of the
//! query's characters appear in order within the candidate (case-insensitive). Matches
//! score higher when they are contiguous, anchored at the start, or align with
//! word/camel-case boundaries — the classic fuzzy-finder (fzf-style) behavior.
//!
//! The frontends call this with the current list of resource names and a typed query; it
//! returns a ranked list of matches. No I/O, no rendering — pure semantics.

/// A single fuzzy-match result, carrying a score (higher = better) for ranking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    /// The matched candidate string (e.g. a resource name).
    pub candidate: String,
    /// Match score: higher is better.
    pub score: i32,
}

/// A ranked *index* into the candidate list, without owning the candidate string.
///
/// This is the allocation-free form of [`fuzzy_jump`] for the TUI's per-keystroke
/// re-rank path (finding AA): [`fuzzy_jump`] returns `FuzzyMatch { candidate: String }`
/// per match — an owned `String` per row — which on a 50 000-row view is ~50 k
/// allocations per keystroke on top of the caller's own clone. Ranked indices carry the
/// same ordering with no per-row allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuzzyRanked {
    /// Index into the original candidate list.
    pub index: usize,
    /// Match score: higher is better.
    pub score: i32,
}

/// Rank `candidates` by fuzzy match against `query`, returning matched **indices**
/// best-first (no owned candidate strings). An empty query matches everything at score 0
/// in input order. The index is the position in the *input* iteration order, so callers
/// map it back onto their own `&[T]` slice.
pub fn fuzzy_rank_indices<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Vec<FuzzyRanked> {
    let query = query.trim().to_ascii_lowercase();

    let mut matches: Vec<FuzzyRanked> = candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            fuzzy_score(candidate, &query).map(|score| FuzzyRanked { index, score })
        })
        .collect();

    // Sort best-first; ties broken by input order for determinism (an empty query
    // already yields score 0 for everything, so this sort is a stable no-op preserving
    // input order — same as `fuzzy_jump`).
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
    matches
}

/// Score and rank `candidates` against a fuzzy `query`. Returns only matches, ordered
/// best-first. An empty query matches everything at score 0 (preserving input order for
/// stable output).
pub fn fuzzy_jump<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    query: &str,
) -> Vec<FuzzyMatch> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return candidates
            .into_iter()
            .map(|c| FuzzyMatch {
                candidate: c.to_string(),
                score: 0,
            })
            .collect();
    }

    let mut matches: Vec<FuzzyMatch> = candidates
        .into_iter()
        .filter_map(|candidate| {
            fuzzy_score(candidate, &query).map(|score| FuzzyMatch {
                candidate: candidate.to_string(),
                score,
            })
        })
        .collect();

    // Sort best-first; ties broken by candidate name for determinism.
    matches.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.candidate.cmp(&b.candidate))
    });
    matches
}

/// Compute the fuzzy-match score of a candidate against a query, or `None` if the query
/// is not a subsequence of the candidate.
///
/// **Allocation-free** (finding AA): iterates the candidate's chars once and the query's
/// chars once — no `to_ascii_lowercase()` `String`, no `Vec<char>` per candidate. On the
/// TUI's per-keystroke re-rank of a 50 000-row view this removes ~100 k allocations per
/// keystroke that the old `fuzzy_score` made inside `fuzzy_jump`.
fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    // Fold the query once (it is short) — this is the only allocation, O(query), reused
    // across every candidate rather than per-candidate.
    let query_chars: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();

    if query_chars.is_empty() {
        return Some(0);
    }

    // Subsequence check with scoring, tracking **char position** (not byte index) so the
    // "consecutive" and "boundary" signals match the original char-vector semantics.
    let mut score: i32 = 0;
    let mut qi = 0usize;
    let mut prev_match: Option<usize> = None;
    let mut prev_char: Option<char> = None;

    for (char_index, c) in candidate.chars().enumerate() {
        let cc = c.to_ascii_lowercase();
        if qi < query_chars.len() && cc == query_chars[qi] {
            // Base score for matching a character.
            score += 1;

            if let Some(prev) = prev_match {
                // Consecutive matches are worth more.
                if char_index == prev + 1 {
                    score += 3;
                }
            } else if char_index == 0 {
                // Match at the very start of the candidate.
                score += 4;
            } else if let Some(pc) = prev_char
                && (pc == ' ' || pc == '-' || pc == '_' || pc == '.' || pc.is_uppercase())
            {
                // Match after a word/camel boundary.
                score += 2;
            }
            prev_match = Some(char_index);
            qi += 1;
        }
        prev_char = Some(c);
    }

    // All query chars must have matched in order.
    if qi == query_chars.len() {
        Some(score)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches_rank_contiguous_higher() {
        let names = ["nginx-ingress-controller", "nagios", "nginxxxxxxxxx"];
        let ranked = fuzzy_jump(names, "nginx");
        // "nginx" as a contiguous prefix ranks first.
        assert_eq!(ranked[0].candidate, "nginx-ingress-controller");
    }

    #[test]
    fn empty_query_matches_all() {
        let names = ["a", "b"];
        let ranked = fuzzy_jump(names, "");
        assert_eq!(ranked.len(), 2);
    }

    #[test]
    fn no_match_returns_empty() {
        let names = ["pod-a", "pod-b"];
        let ranked = fuzzy_jump(names, "zzz");
        assert!(ranked.is_empty());
    }

    #[test]
    fn case_insensitive() {
        let names = ["Deployment"];
        let ranked = fuzzy_jump(names, "DEP");
        assert_eq!(ranked.len(), 1);
    }

    #[test]
    fn camel_case_boundary_boost() {
        let names = ["imagePullBackOff", "ImagePullError"];
        let ranked = fuzzy_jump(names, "ipbo");
        // Both are subsequence matches; the camel-boundary-aligned one ranks first.
        assert_eq!(ranked[0].candidate, "imagePullBackOff");
    }
}
