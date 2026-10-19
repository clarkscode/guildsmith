//! "Did you mean …?" suggestions, shared by validation and name resolution.

/// Case-insensitive Levenshtein distance.
#[must_use]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Up to `n` candidates close to `input`, closest first. "Close" scales with input length.
#[must_use]
pub fn closest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>, n: usize) -> Vec<String> {
    let limit = (input.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, &str)> =
        candidates.into_iter().map(|c| (edit_distance(input, c), c)).filter(|(d, _)| *d <= limit).collect();
    scored.sort_unstable();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(n).map(|(_, c)| c.to_owned()).collect()
}
