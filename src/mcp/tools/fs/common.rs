use anyhow::{self as ah, Context as _};
use std::path::{Path, PathBuf};
use tokio::fs;

pub const READ_FILE_MAX_SIZE: u64 = 256 * 1024;
pub const MAX_DIR_ENTRIES: usize = 16 * 1024;
pub const MAX_GREP_DIR_MATCHES: usize = 500;
pub const MAX_GREP_DIR_FILES: usize = 10_000;
pub const MAX_GREP_DIR_RESULT_SIZE: usize = 8 * 1024 * 1024;
pub const MAX_FIND_FILES: usize = 10_000;

type GrepRanges = (Vec<(usize, usize)>, std::collections::HashSet<usize>);

/// Computes the merged context ranges and match-line index set for a grep over `lines`.
/// Returns `None` if no lines match.
pub fn compute_grep_matches(lines: &[&str], re: &regex::Regex, ctx: usize) -> Option<GrepRanges> {
    let matching: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| re.is_match(line))
        .map(|(i, _)| i)
        .collect();
    if matching.is_empty() {
        return None;
    }
    let mut ranges: Vec<(usize, usize)> = vec![];
    for &m in &matching {
        let start = m.saturating_sub(ctx);
        let end = (m + ctx).min(lines.len().saturating_sub(1));
        if let Some(last) = ranges.last_mut()
            && start <= last.1 + 1
        {
            last.1 = last.1.max(end);
            continue;
        }
        ranges.push((start, end));
    }
    let match_set = matching.into_iter().collect();
    Some((ranges, match_set))
}

/// Formats grep ranges into `out`. Calls `check_limit(is_match, out_len)` after each line;
/// returns `true` if the limit callback signalled a stop.
pub fn format_grep_ranges(
    lines: &[&str],
    ranges: &[(usize, usize)],
    match_set: &std::collections::HashSet<usize>,
    out: &mut String,
    mut check_limit: impl FnMut(bool, usize) -> bool,
) -> bool {
    let mut first = true;
    for &(start, end) in ranges {
        if !first {
            out.push_str("--\n");
        }
        first = false;
        for (i, line) in lines
            .iter()
            .enumerate()
            .take(end.saturating_add(1))
            .skip(start)
        {
            let nr = i.saturating_add(1);
            let is_match = match_set.contains(&i);
            let sep = if is_match { ':' } else { '-' };
            out.push_str(&format!("{nr}{sep}{line}\n"));
            if check_limit(is_match, out.len()) {
                return true;
            }
        }
    }
    false
}

/// Canonicalizes a path, resolving symlinks and relative components.
pub async fn canonicalize(path: &Path) -> ah::Result<PathBuf> {
    fs::canonicalize(path)
        .await
        .with_context(|| format!("Failed to canonicalize path `{}`", path.display()))
}
