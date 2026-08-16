//! One file's diff, for the right pane's second view (SPEC.md §5.4, §8.8).
//!
//! Only *parsing* lives here. Turning a unified hunk into two aligned columns is
//! rendering work and lives in `diffLayout.ts` beside the component that draws
//! it — the same split as `graph.rs` / `graphLayout.ts` (§5.3).
//!
//! Every command below is read-only, so they all go through `git::read` (§3).
//! Nothing here fetches, and nothing here may ever take the write path.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;

/// Which two sides to compare (§5.4). The frontend picks this from the section
/// the file row was clicked in, because that is the only thing that knows it:
/// the same path can sit in *Staged Changes* and *Changes* at once with a
/// different diff on each side.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DiffSource {
    /// Worktree vs index — a row in *Changes*.
    Unstaged,
    /// Index vs HEAD — a row in *Staged Changes*.
    Staged,
    /// A `?` row. Git has nothing to diff against, so this one never shells
    /// out; see `read_untracked`.
    Untracked,
    /// A commit vs its parent — a row in the commit info panel (§5.2 Mode B).
    Commit { hash: String },
}

/// Past this many body lines the diff stops being something a human reads and
/// starts being a way to put 400k DOM rows on screen. Parsing stops there and
/// `truncated` sends the user to VS Code instead (§5.4) — the same escape
/// hatch as binary and conflicted files.
const MAX_DIFF_LINES: usize = 20_000;

/// The equivalent cap for an untracked file, which is measured before reading
/// rather than after: `read_untracked` loads the whole file into memory, so the
/// line cap alone would still mean reading a 2 GB build artefact first.
const MAX_UNTRACKED_BYTES: u64 = 5 * 1024 * 1024;

/// How much of an untracked file to scan for a NUL before calling it binary.
/// Git's own heuristic looks at the first 8000 bytes; matching it keeps our
/// answer the same as the one `git diff` would have given.
const BINARY_SNIFF_BYTES: usize = 8000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
    /// Git said "Binary files … differ", or an untracked file had a NUL in it.
    /// There is nothing to render side by side; the view offers VS Code.
    pub binary: bool,
    /// Hit `MAX_DIFF_LINES` (or `MAX_UNTRACKED_BYTES`) — the hunks present are
    /// real but incomplete, and the view must say so rather than imply the
    /// file ends there.
    pub truncated: bool,
    pub insertions: u32,
    pub deletions: u32,
}

/// One `@@` block. Start lines are 1-based, as git prints them; a count of 0
/// (a pure insertion into an empty side) is legal and means the start line is
/// the line *before* the change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

/// `kind` is git's own leading character — `' '`, `'+'` or `'-'` — kept as-is
/// rather than mapped to an enum, so the parser stays a transcription of what
/// git printed and `diffLayout.ts` does the interpreting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: char,
    pub text: String,
}

/// The one entry point. `path` is repo-relative and forward-slashed, exactly as
/// `status::query_files` and `graph::details` hand it to the frontend.
pub async fn file(repo: &Path, path: &str, source: &DiffSource) -> Result<FileDiff, String> {
    if matches!(source, DiffSource::Untracked) {
        return read_untracked(repo, path).await;
    }

    // `--no-ext-diff` because a user's configured external diff driver would
    // otherwise print something we have no parser for — and, worse, could be an
    // interactive tool. `--no-color` because `color.diff=always` in a user's
    // config would salt the output with escape sequences. `--no-renames`
    // because the pathspec already pins exactly one path, and half a rename
    // pair reads worse than the add/delete pair git falls back to.
    let common = ["--no-ext-diff", "--no-color", "--no-renames", "-U3"];

    let args: Vec<&str> = match source {
        DiffSource::Unstaged => {
            let mut args = vec!["diff"];
            args.extend_from_slice(&common);
            args.extend_from_slice(&["--", path]);
            args
        }
        DiffSource::Staged => {
            let mut args = vec!["diff", "--cached"];
            args.extend_from_slice(&common);
            args.extend_from_slice(&["--", path]);
            args
        }
        // Deliberately the same `diff-tree` family `graph::details` builds the
        // file list from, so the diff and the list it was clicked in can never
        // disagree — including agreeing to show nothing for a merge commit,
        // which `diff-tree` omits by default.
        DiffSource::Commit { hash } => {
            let mut args = vec!["diff-tree", "--no-commit-id", "-p", "-r"];
            args.extend_from_slice(&common);
            args.extend_from_slice(&[hash.as_str(), "--", path]);
            args
        }
        DiffSource::Untracked => unreachable!("handled above"),
    };

    let output = git::read(repo, &args).await?;
    if !output.ok {
        return Err(full_message(&output.stderr));
    }

    Ok(parse_patch(path, &output.stdout))
}

/// An untracked file never shells out. `git diff` reports nothing at all for
/// one, and `git diff --no-index` deliberately exits 1 when the files differ —
/// which `git::read` correctly reports as a failure. Reading the file and
/// calling every line an addition is both simpler and honest: that *is* the
/// diff against nothing.
async fn read_untracked(repo: &Path, path: &str) -> Result<FileDiff, String> {
    let full = repo.join(path);

    // Checked before reading, not after: the line cap below would still mean
    // pulling a 2 GB build artefact through memory first.
    match tokio::fs::metadata(&full).await {
        Ok(meta) if meta.len() > MAX_UNTRACKED_BYTES => {
            return Ok(FileDiff { path: path.to_string(), truncated: true, ..FileDiff::default() });
        }
        Ok(_) => {}
        Err(err) => return Err(format!("could not read {path}: {err}")),
    }

    let bytes = tokio::fs::read(&full)
        .await
        .map_err(|err| format!("could not read {path}: {err}"))?;

    if bytes.iter().take(BINARY_SNIFF_BYTES).any(|&b| b == 0) {
        return Ok(FileDiff { path: path.to_string(), binary: true, ..FileDiff::default() });
    }

    let text = String::from_utf8_lossy(&bytes);
    let truncated = text.lines().count() > MAX_DIFF_LINES;
    let lines: Vec<DiffLine> = text
        .lines()
        .take(MAX_DIFF_LINES)
        .map(|line| DiffLine { kind: '+', text: line.to_string() })
        .collect();

    let count = lines.len() as u32;
    let hunks = if count == 0 {
        Vec::new()
    } else {
        // The whole file as one hunk, starting at old line 0 because there is
        // no old side at all — the same shape git prints for a new file.
        vec![DiffHunk { old_start: 0, old_count: 0, new_start: 1, new_count: count, lines }]
    };

    Ok(FileDiff { path: path.to_string(), hunks, binary: false, truncated, insertions: count, deletions: 0 })
}

/// `str::lines` splits on `\n` and drops a trailing `\r`, which is exactly what
/// we want: a CRLF file's diff would otherwise carry a stray carriage return
/// into every rendered line. The cost is that a pure line-ending change reads
/// as identical text on both sides — true of VS Code's diff too, and the
/// alternative is a column of visible control characters.
fn parse_patch(path: &str, raw: &str) -> FileDiff {
    let mut diff = FileDiff { path: path.to_string(), ..FileDiff::default() };
    let mut hunk: Option<DiffHunk> = None;
    // Counts down the `@@` header's own promise of how many lines each side
    // has left. Tracking it (rather than stopping at the first unrecognised
    // character) is what lets a genuinely empty context line — which git prints
    // as a bare `""`, not `" "` — be told apart from the end of the hunk.
    let mut old_left = 0u32;
    let mut new_left = 0u32;
    let mut body_lines = 0usize;

    for line in raw.lines() {
        if let Some(header) = line.strip_prefix("@@") {
            if let Some(finished) = hunk.take() {
                diff.hunks.push(finished);
            }
            let Some((old_start, old_count, new_start, new_count)) = parse_hunk_header(header)
            else {
                continue;
            };
            old_left = old_count;
            new_left = new_count;
            hunk = Some(DiffHunk { old_start, old_count, new_start, new_count, lines: Vec::new() });
            continue;
        }

        let Some(current) = hunk.as_mut() else {
            // Still in the file header. `Binary files a/x and b/x differ` is the
            // only part of it we care about; `diff --git`, `index`, `---`/`+++`
            // and mode lines all say things the caller already knows.
            if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
                diff.binary = true;
            }
            continue;
        };

        // `\ No newline at end of file` annotates the line above rather than
        // being one. Corgit does not render the distinction, so it is dropped.
        if line.starts_with('\\') {
            continue;
        }

        if old_left == 0 && new_left == 0 {
            // The hunk's line budget is spent, so this belongs to whatever
            // follows it — a second file's header in a multi-file patch.
            diff.hunks.push(hunk.take().expect("checked above"));
            if line.starts_with("Binary files ") {
                diff.binary = true;
            }
            continue;
        }

        let (kind, text) = match line.chars().next() {
            Some('+') => ('+', &line[1..]),
            Some('-') => ('-', &line[1..]),
            Some(' ') => (' ', &line[1..]),
            // A bare empty line is git's rendering of an empty context line.
            None => (' ', ""),
            _ => continue,
        };

        match kind {
            '+' => {
                new_left = new_left.saturating_sub(1);
                diff.insertions += 1;
            }
            '-' => {
                old_left = old_left.saturating_sub(1);
                diff.deletions += 1;
            }
            _ => {
                old_left = old_left.saturating_sub(1);
                new_left = new_left.saturating_sub(1);
            }
        }

        current.lines.push(DiffLine { kind, text: text.to_string() });
        body_lines += 1;
        if body_lines >= MAX_DIFF_LINES {
            diff.truncated = true;
            break;
        }
    }

    if let Some(finished) = hunk.take() {
        diff.hunks.push(finished);
    }
    diff
}

/// `@@ -12,7 +12,9 @@ optional section heading` — with the leading `@@`
/// already stripped. A count may be omitted (`-1 +1`), which means 1; a count
/// of 0 is legal and means the change is an insertion at that boundary.
fn parse_hunk_header(header: &str) -> Option<(u32, u32, u32, u32)> {
    let body = header.split("@@").next()?;
    let mut ranges = body.split_whitespace();
    let (old_start, old_count) = parse_range(ranges.next()?.strip_prefix('-')?)?;
    let (new_start, new_count) = parse_range(ranges.next()?.strip_prefix('+')?)?;
    Some((old_start, old_count, new_start, new_count))
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// The whole trimmed stderr, not just its first line — §13's "raw stderr always
/// available in a collapsible Details" needs the whole thing; the frontend's
/// `translateGitError` picks a plain-language headline out of it.
fn full_message(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() { "git diff failed".to_string() } else { trimmed.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: char, text: &str) -> DiffLine {
        DiffLine { kind, text: text.to_string() }
    }

    #[test]
    fn parses_a_single_hunk() {
        let raw = "diff --git a/src/main.rs b/src/main.rs\n\
                   index 1234567..89abcde 100644\n\
                   --- a/src/main.rs\n\
                   +++ b/src/main.rs\n\
                   @@ -12,4 +12,5 @@ fn main() {\n\
                   \x20    let a = 1;\n\
                   -    old();\n\
                   +    new();\n\
                   +    extra();\n\
                   \x20    let b = 2;\n";

        let diff = parse_patch("src/main.rs", raw);

        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.insertions, 2);
        assert_eq!(diff.deletions, 1);
        let hunk = &diff.hunks[0];
        assert_eq!((hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count), (12, 4, 12, 5));
        assert_eq!(
            hunk.lines,
            vec![
                line(' ', "    let a = 1;"),
                line('-', "    old();"),
                line('+', "    new();"),
                line('+', "    extra();"),
                line(' ', "    let b = 2;"),
            ]
        );
    }

    #[test]
    fn parses_several_hunks() {
        let raw = "@@ -1,2 +1,2 @@\n\
                   -a\n\
                   +A\n\
                   \x20b\n\
                   @@ -20,2 +20,2 @@\n\
                   -c\n\
                   +C\n\
                   \x20d\n";

        let diff = parse_patch("f.txt", raw);

        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[1].old_start, 20);
        assert_eq!(diff.insertions, 2);
        assert_eq!(diff.deletions, 2);
    }

    /// The one case a "stop at the first unrecognised character" parser gets
    /// wrong: git prints an empty context line as a bare empty line, not as a
    /// single space, so it is indistinguishable from a blank separator except
    /// by the hunk header's line budget.
    #[test]
    fn an_empty_context_line_stays_inside_the_hunk() {
        let raw = "@@ -1,3 +1,3 @@\n\
                   -a\n\
                   \n\
                   +A\n";

        let diff = parse_patch("f.txt", raw);

        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines, vec![line('-', "a"), line(' ', ""), line('+', "A")]);
    }

    #[test]
    fn a_new_file_has_no_old_side() {
        let raw = "diff --git a/new.txt b/new.txt\n\
                   new file mode 100644\n\
                   index 0000000..1234567\n\
                   --- /dev/null\n\
                   +++ b/new.txt\n\
                   @@ -0,0 +1,2 @@\n\
                   +one\n\
                   +two\n";

        let diff = parse_patch("new.txt", raw);

        let hunk = &diff.hunks[0];
        assert_eq!((hunk.old_start, hunk.old_count), (0, 0));
        assert_eq!(diff.insertions, 2);
        assert_eq!(diff.deletions, 0);
    }

    #[test]
    fn a_deleted_file_has_no_new_side() {
        let raw = "@@ -1,2 +0,0 @@\n-one\n-two\n";
        let diff = parse_patch("gone.txt", raw);

        assert_eq!(diff.hunks[0].new_count, 0);
        assert_eq!(diff.deletions, 2);
        assert_eq!(diff.insertions, 0);
    }

    #[test]
    fn a_header_with_omitted_counts_means_one_line() {
        let raw = "@@ -1 +1 @@\n-a\n+b\n";
        let diff = parse_patch("f.txt", raw);

        let hunk = &diff.hunks[0];
        assert_eq!((hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count), (1, 1, 1, 1));
        assert_eq!(hunk.lines.len(), 2);
    }

    #[test]
    fn a_binary_file_is_flagged_and_has_no_hunks() {
        let raw = "diff --git a/logo.png b/logo.png\n\
                   index 1234567..89abcde 100644\n\
                   Binary files a/logo.png and b/logo.png differ\n";

        let diff = parse_patch("logo.png", raw);

        assert!(diff.binary);
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn a_no_newline_marker_is_not_a_line() {
        let raw = "@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n";
        let diff = parse_patch("f.txt", raw);

        assert_eq!(diff.hunks[0].lines, vec![line('-', "a"), line('+', "b")]);
    }

    /// `str::lines` drops the `\r`, so a CRLF-terminated patch must parse
    /// identically to an LF one rather than carrying a control character into
    /// every rendered line.
    #[test]
    fn crlf_output_parses_the_same_as_lf() {
        let lf = parse_patch("f.txt", "@@ -1 +1 @@\n-a\n+b\n");
        let crlf = parse_patch("f.txt", "@@ -1 +1 @@\r\n-a\r\n+b\r\n");

        assert_eq!(lf, crlf);
    }

    #[test]
    fn empty_output_is_no_changes_rather_than_an_error() {
        let diff = parse_patch("f.txt", "");

        assert!(diff.hunks.is_empty());
        assert!(!diff.binary);
        assert!(!diff.truncated);
    }

    #[test]
    fn a_diff_past_the_cap_is_truncated_rather_than_rendered() {
        let body: String = (0..MAX_DIFF_LINES + 500).map(|n| format!("+line {n}\n")).collect();
        let raw = format!("@@ -0,0 +1,{} @@\n{body}", MAX_DIFF_LINES + 500);

        let diff = parse_patch("huge.txt", &raw);

        assert!(diff.truncated);
        assert_eq!(diff.hunks[0].lines.len(), MAX_DIFF_LINES);
    }

    #[test]
    fn a_hunk_header_that_is_not_one_is_skipped_rather_than_panicking() {
        assert!(parse_hunk_header(" nonsense @@").is_none());
        assert!(parse_hunk_header(" -x,y +1,2 @@").is_none());
        assert!(parse_patch("f.txt", "@@ garbage @@\n+a\n").hunks.is_empty());
    }
}
