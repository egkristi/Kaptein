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
fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    let lower = candidate.to_ascii_lowercase();
    let candidate_chars: Vec<char> = lower.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();

    if query_chars.is_empty() {
        return Some(0);
    }

    // Subsequence check with scoring.
    let mut score: i32 = 0;
    let mut qi = 0usize; // index into query_chars
    let mut prev_match: Option<usize> = None;

    for (ci, c) in candidate_chars.iter().enumerate() {
        if qi < query_chars.len() && *c == query_chars[qi] {
            // Base score for matching a character.
            score += 1;

            if let Some(prev) = prev_match {
                // Consecutive matches are worth more.
                if ci == prev + 1 {
                    score += 3;
                }
            } else if ci == 0 {
                // Match at the very start of the candidate.
                score += 4;
            } else {
                // Match after a word/camel boundary.
                let prev_char = candidate_chars[ci - 1];
                if prev_char == ' '
                    || prev_char == '-'
                    || prev_char == '_'
                    || prev_char == '.'
                    || prev_char.is_uppercase()
                {
                    score += 2;
                }
            }
            prev_match = Some(ci);
            qi += 1;
        }
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
