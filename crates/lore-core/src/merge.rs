/// Line-level 3-way merge for plain-text / markdown note content.
///
/// Takes three versions of a document and produces a merged result:
/// - `base`   — common ancestor (last version both sides agreed on)
/// - `ours`   — our offline edits
/// - `theirs` — concurrent changes written to the server while we were offline
///
/// Non-overlapping edits from both sides are applied automatically.
/// When both sides touch the same region differently, both versions are kept
/// (ours first, then theirs) separated by a blank line.

#[derive(Debug, PartialEq)]
pub struct MergeResult {
    pub text: String,
    /// True if at least one hunk had conflicting edits from both sides.
    pub had_conflict: bool,
}

pub fn three_way_merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    if ours == theirs {
        return MergeResult {
            text: ours.to_string(),
            had_conflict: false,
        };
    }
    if ours == base {
        return MergeResult {
            text: theirs.to_string(),
            had_conflict: false,
        };
    }
    if theirs == base {
        return MergeResult {
            text: ours.to_string(),
            had_conflict: false,
        };
    }

    let base_l: Vec<&str> = base.lines().collect();
    let ours_l: Vec<&str> = ours.lines().collect();
    let theirs_l: Vec<&str> = theirs.lines().collect();

    let ours_hunks = diff(&base_l, &ours_l);
    let theirs_hunks = diff(&base_l, &theirs_l);

    apply_three_way(&base_l, &ours_hunks, &theirs_hunks)
}

// ---- Diff ---------------------------------------------------------------

/// A replacement hunk: base[base_start..base_end] → lines.
/// An insertion has base_start == base_end; a deletion has lines.is_empty().
#[derive(Debug, Clone)]
struct Hunk<'a> {
    base_start: usize,
    base_end: usize,
    lines: Vec<&'a str>,
}

/// Compute hunks that transform `base` into `changed`.
fn diff<'a>(base: &[&str], changed: &'a [&str]) -> Vec<Hunk<'a>> {
    let pairs = lcs_pairs(base, changed);
    let mut hunks = Vec::new();
    let mut prev_bi = 0usize;
    let mut prev_ci = 0usize;

    for &(bi, ci) in &pairs {
        if prev_bi < bi || prev_ci < ci {
            hunks.push(Hunk {
                base_start: prev_bi,
                base_end: bi,
                lines: changed[prev_ci..ci].to_vec(),
            });
        }
        prev_bi = bi + 1;
        prev_ci = ci + 1;
    }
    // Trailing gap after last match.
    if prev_bi < base.len() || prev_ci < changed.len() {
        hunks.push(Hunk {
            base_start: prev_bi,
            base_end: base.len(),
            lines: changed[prev_ci..].to_vec(),
        });
    }
    hunks
}

/// LCS as matching (base_idx, changed_idx) pairs, in order.
fn lcs_pairs<T: Eq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut pairs = Vec::with_capacity(dp[0][0] as usize);
    let (mut i, mut j) = (0, 0);
    while i < m && j < n {
        if a[i] == b[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

// ---- 3-way application --------------------------------------------------

fn apply_three_way<'a>(base: &[&'a str], ours: &[Hunk<'a>], theirs: &[Hunk<'a>]) -> MergeResult {
    let mut result: Vec<&str> = Vec::new();
    let mut had_conflict = false;
    let mut oi = 0usize;
    let mut ti = 0usize;
    let mut base_pos = 0usize;

    loop {
        let oh = ours.get(oi);
        let th = theirs.get(ti);

        match (oh, th) {
            (None, None) => break,
            (Some(h), None) => {
                emit_base(base, base_pos, h.base_start, &mut result);
                result.extend_from_slice(&h.lines);
                base_pos = h.base_end;
                oi += 1;
            }
            (None, Some(h)) => {
                emit_base(base, base_pos, h.base_start, &mut result);
                result.extend_from_slice(&h.lines);
                base_pos = h.base_end;
                ti += 1;
            }
            (Some(oh), Some(th)) => {
                // Two pure insertions at the same base point must be treated
                // as overlapping (both sides inserted at the same location).
                let same_point_insert = oh.base_start == oh.base_end
                    && th.base_start == th.base_end
                    && oh.base_start == th.base_start;

                if oh.base_end <= th.base_start && !same_point_insert {
                    // Ours is strictly before theirs — no overlap.
                    emit_base(base, base_pos, oh.base_start, &mut result);
                    result.extend_from_slice(&oh.lines);
                    base_pos = oh.base_end;
                    oi += 1;
                } else if th.base_end <= oh.base_start && !same_point_insert {
                    // Theirs is strictly before ours — no overlap.
                    emit_base(base, base_pos, th.base_start, &mut result);
                    result.extend_from_slice(&th.lines);
                    base_pos = th.base_end;
                    ti += 1;
                } else {
                    // Overlapping region.
                    let region_start = oh.base_start.min(th.base_start);
                    let region_end = oh.base_end.max(th.base_end);
                    emit_base(base, base_pos, region_start, &mut result);

                    if oh.lines == th.lines {
                        // Convergent edit — identical result, no conflict.
                        result.extend_from_slice(&oh.lines);
                    } else {
                        had_conflict = true;
                        result.extend_from_slice(&oh.lines);
                        if !oh.lines.is_empty() && !th.lines.is_empty() {
                            result.push("");
                        }
                        result.extend_from_slice(&th.lines);
                    }

                    base_pos = region_end;
                    let oi_before = oi;
                    let ti_before = ti;
                    while oi < ours.len() && ours[oi].base_start < region_end {
                        oi += 1;
                    }
                    while ti < theirs.len() && theirs[ti].base_start < region_end {
                        ti += 1;
                    }
                    // Pure insertions at the boundary (base_start == base_end ==
                    // region_end) never satisfy `< region_end` — advance manually
                    // so the loop doesn't stall on the same pair forever.
                    if oi == oi_before {
                        oi += 1;
                    }
                    if ti == ti_before {
                        ti += 1;
                    }
                }
            }
        }
    }

    emit_base(base, base_pos, base.len(), &mut result);

    MergeResult {
        text: result.join("\n"),
        had_conflict,
    }
}

fn emit_base<'a>(base: &[&'a str], from: usize, to: usize, out: &mut Vec<&'a str>) {
    out.extend_from_slice(&base[from..to]);
}

// ---- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn m(base: &str, ours: &str, theirs: &str) -> MergeResult {
        three_way_merge(base, ours, theirs)
    }

    #[test]
    fn identical_inputs() {
        let r = m("a\nb", "a\nb", "a\nb");
        assert_eq!(r.text, "a\nb");
        assert!(!r.had_conflict);
    }

    #[test]
    fn only_ours_changed() {
        let r = m("hello", "hello world", "hello");
        assert_eq!(r.text, "hello world");
        assert!(!r.had_conflict);
    }

    #[test]
    fn only_theirs_changed() {
        let r = m("hello", "hello", "hello world");
        assert_eq!(r.text, "hello world");
        assert!(!r.had_conflict);
    }

    #[test]
    fn non_overlapping_ours_prepends_theirs_appends() {
        let r = m("middle", "top\nmiddle", "middle\nbottom");
        assert_eq!(r.text, "top\nmiddle\nbottom");
        assert!(!r.had_conflict);
    }

    #[test]
    fn convergent_edit_no_conflict() {
        let r = m("old", "new", "new");
        assert_eq!(r.text, "new");
        assert!(!r.had_conflict);
    }

    #[test]
    fn overlapping_conflict_contains_both_versions() {
        let r = m("original", "ours version", "theirs version");
        assert!(r.had_conflict);
        assert!(
            r.text.contains("ours version"),
            "missing ours: {:?}",
            r.text
        );
        assert!(
            r.text.contains("theirs version"),
            "missing theirs: {:?}",
            r.text
        );
    }

    #[test]
    fn theirs_appends_section_ours_edits_title() {
        let base = "Title\n\nBody text";
        let ours = "New Title\n\nBody text";
        let theirs = "Title\n\nBody text\n\n## New Section\nAdded by worker";
        let r = m(base, ours, theirs);
        assert!(!r.had_conflict, "should merge cleanly; got: {:?}", r.text);
        assert!(r.text.contains("New Title"));
        assert!(r.text.contains("## New Section"));
    }

    #[test]
    fn both_add_different_lines_at_end() {
        let base = "line1\nline2";
        let ours = "line1\nline2\nours added";
        let theirs = "line1\nline2\ntheirs added";
        let r = m(base, ours, theirs);
        // Both added at the same position — conflict.
        assert!(r.had_conflict);
        assert!(r.text.contains("ours added"));
        assert!(r.text.contains("theirs added"));
    }

    #[test]
    fn multiline_ours_delete_theirs_append() {
        let base = "a\nb\nc";
        let ours = "a\nc"; // deleted b
        let theirs = "a\nb\nc\nd"; // appended d
        let r = m(base, ours, theirs);
        assert!(!r.had_conflict);
        assert!(r.text.contains('a'));
        assert!(!r.text.contains('b'), "b was deleted by ours");
        assert!(r.text.contains('c'));
        assert!(r.text.contains('d'));
    }

    // ---- Exact-output tests for the diff / LCS / apply core ----------------
    //
    // The properties and `contains()` assertions above never pin the precise
    // merged text, and the identity laws short-circuit on the `ours == base` /
    // `theirs == base` early returns — so they never exercise `diff`,
    // `lcs_pairs`, or `apply_three_way` at all. These cases use three genuinely
    // distinct documents (so the algorithm runs) and assert the *exact* result,
    // which is what makes a wrong LCS, a dropped trailing hunk, a broken
    // hunk-advance, or a misplaced conflict separator observable.

    /// One side deletes the tail of the document while the other edits the
    /// head. Pins the trailing-gap branch of `diff` (a dropped trailing
    /// deletion would leave `c`/`d` behind).
    #[test]
    fn trailing_deletion_on_one_side() {
        let r = m("a\nb\nc\nd", "a\nb", "z\nb\nc\nd");
        assert_eq!(r.text, "z\nb");
        assert!(!r.had_conflict);
    }

    /// One side appends past the end of base while the other edits the head.
    /// Drives `lcs_pairs` traceback right up to `i == base.len()` with the
    /// changed side still having lines left (the `i < m` bound).
    #[test]
    fn trailing_insertion_on_one_side() {
        let r = m("a\nb", "a\nb\nc", "z\nb");
        assert_eq!(r.text, "z\nb\nc");
        assert!(!r.had_conflict);
    }

    /// Two separate, non-adjacent edits from one side plus one edit from the
    /// other, none overlapping. Forces the hunk index to advance across
    /// multiple hunks (`oi += 1` / `ti += 1`).
    #[test]
    fn multiple_non_overlapping_hunks_from_one_side() {
        let base = "a\nb\nc\nd\ne";
        let ours = "X\nb\nY\nd\ne"; // edit line 0 and line 2 (two hunks)
        let theirs = "a\nb\nc\nd\nZ"; // edit line 4 (one hunk)
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "X\nb\nY\nd\nZ");
        assert!(!r.had_conflict);
    }

    /// Interleaved single-line edits from both sides at distinct positions —
    /// the LCS must keep every unchanged line in place. A wrong dp recurrence
    /// or traceback direction reorders or drops a line here.
    #[test]
    fn interleaved_edits_both_sides_clean() {
        let base = "1\n2\n3\n4\n5";
        let ours = "1\nX\n3\n4\n5"; // edit line 1
        let theirs = "1\n2\n3\nY\n5"; // edit line 3
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "1\nX\n3\nY\n5");
        assert!(!r.had_conflict);
    }

    /// Both sides rewrite the same line differently → conflict keeps both,
    /// separated by a blank line (both replacement texts non-empty).
    #[test]
    fn conflict_keeps_both_with_blank_separator() {
        let r = m("orig", "mine", "yours");
        assert_eq!(r.text, "mine\n\nyours");
        assert!(r.had_conflict);
    }

    /// One side deletes a line, the other edits the same line → conflict, but
    /// the deletion side is empty so there must be NO blank separator before
    /// the surviving edit. Pins the `!ours.is_empty() && !theirs.is_empty()`
    /// guard (left operand).
    #[test]
    fn conflict_delete_vs_edit_no_spurious_blank() {
        let base = "a\nMID\nb";
        let ours = "a\nb"; // delete MID
        let theirs = "a\nEDIT\nb"; // edit MID
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "a\nEDIT\nb");
        assert!(r.had_conflict);
    }

    /// Mirror of the above: ours edits, theirs deletes. Pins the right operand
    /// of the same blank-separator guard.
    #[test]
    fn conflict_edit_vs_delete_no_spurious_blank() {
        let base = "a\nMID\nb";
        let ours = "a\nEDIT\nb"; // edit MID
        let theirs = "a\nb"; // delete MID
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "a\nEDIT\nb");
        assert!(r.had_conflict);
    }

    /// Both sides make the *same* edit to the same region — convergent, so the
    /// region is emitted once with no conflict. Distinct from the early-return
    /// `ours == theirs` fast path because the documents differ elsewhere too is
    /// not needed here; the region-merge path still runs because neither side
    /// equals base.
    #[test]
    fn convergent_same_region_edit_no_conflict() {
        let r = m("a\nMID\nb", "a\nSAME\nb", "a\nSAME\nb");
        assert_eq!(r.text, "a\nSAME\nb");
        assert!(!r.had_conflict);
    }

    /// Adjacent edits where ours is strictly before theirs in base order —
    /// exercises the "ours strictly before theirs, no overlap" branch and the
    /// `oh.base_end <= th.base_start` comparison.
    #[test]
    fn ours_region_strictly_before_theirs() {
        let base = "a\nb\nc\nd";
        let ours = "A\nb\nc\nd"; // edit line 0
        let theirs = "a\nb\nc\nD"; // edit line 3
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "A\nb\nc\nD");
        assert!(!r.had_conflict);
    }

    /// Repeated lines in base so the LCS has more than one valid alignment;
    /// the algorithm must still anchor the edit at the right occurrence. A
    /// broken dp picks the wrong anchor and shifts the inserted line.
    #[test]
    fn repeated_lines_anchor_correctly() {
        // base has two "x" lines; theirs inserts between them, ours edits tail.
        let base = "x\nmid\nx\ntail";
        let ours = "x\nmid\nx\nTAIL"; // edit last line
        let theirs = "x\nmid\nINS\nx\ntail"; // insert before second x
        let r = m(base, ours, theirs);
        assert_eq!(r.text, "x\nmid\nINS\nx\nTAIL");
        assert!(!r.had_conflict);
    }
}
