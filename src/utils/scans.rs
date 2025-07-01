use crate::models::scans::DiffHunk;

pub fn should_merge_hunks(hunk1: &DiffHunk, hunk2: &DiffHunk, max_gap: usize) -> bool {
    // Only merge if they're close enough
    if (hunk2.context_start_line as i64 - hunk1.context_end_line as i64).abs() > max_gap as i64 {
        return false;
    }

    // Check for context overlap
    hunk1.context_end_line >= hunk2.context_start_line
        || (hunk2.context_start_line - hunk1.context_end_line) <= max_gap
}
