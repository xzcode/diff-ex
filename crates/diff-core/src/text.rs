//! Line comparison with coordinates in the original documents.
//!
//! Rules change comparison keys only. The returned half-open hunk ranges always
//! address original lines, including ignored blank lines and a final empty line.

use regex::Regex;
use serde::{Deserialize, Serialize};
use similar::{capture_diff_slices, Algorithm, DiffTag};
use std::ops::Range;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum TextAlgorithm {
    #[default]
    Myers,
    Patience,
    Histogram,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Whitespace {
    #[default]
    None,
    Trim,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextOptions {
    pub algorithm: TextAlgorithm,
    pub ignore_case: bool,
    pub whitespace: Whitespace,
    pub ignore_blank_lines: bool,
    pub ignore_line_endings: bool,
    pub ignore_line_prefixes: Vec<String>,
    pub ignore_line_regex: String,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            algorithm: TextAlgorithm::Myers,
            ignore_case: false,
            whitespace: Whitespace::None,
            ignore_blank_lines: false,
            ignore_line_endings: true,
            ignore_line_prefixes: Vec::new(),
            ignore_line_regex: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextHunk {
    pub left_start: usize,
    pub left_end: usize,
    pub right_start: usize,
    pub right_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TextDiffResult {
    pub hunks: Vec<TextHunk>,
    pub equal: bool,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Ending {
    None,
    Lf,
    CrLf,
    Cr,
    Ignored,
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct LineKey {
    text: String,
    ending: Ending,
}

struct Lines {
    keys: Vec<LineKey>,
    original_indexes: Vec<usize>,
}

/// Split all three common newline styles without losing a trailing empty line.
fn original_lines(text: &str) -> Vec<(&str, Ending)> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let ending = match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => Some((Ending::CrLf, 2)),
            b'\r' => Some((Ending::Cr, 1)),
            b'\n' => Some((Ending::Lf, 1)),
            _ => None,
        };
        if let Some((ending, width)) = ending {
            lines.push((&text[start..i], ending));
            i += width;
            start = i;
        } else {
            i += 1;
        }
    }
    lines.push((&text[start..], Ending::None));
    lines
}

fn comparison_lines(text: &str, options: &TextOptions, regex: Option<&Regex>) -> Lines {
    let prefixes: Vec<&str> = options
        .ignore_line_prefixes
        .iter()
        .map(|prefix| prefix.trim())
        .filter(|prefix| !prefix.is_empty())
        .collect();
    let mut keys = Vec::new();
    let mut original_indexes = Vec::new();
    for (index, (line, ending)) in original_lines(text).into_iter().enumerate() {
        if options.ignore_blank_lines && line.trim().is_empty() {
            continue;
        }
        if prefixes
            .iter()
            .any(|prefix| line.trim_start().starts_with(*prefix))
            || regex.is_some_and(|pattern| pattern.is_match(line))
        {
            continue;
        }
        let mut normalized = match options.whitespace {
            Whitespace::None => line.to_owned(),
            Whitespace::Trim => line.trim().to_owned(),
            Whitespace::All => line.chars().filter(|c| !c.is_whitespace()).collect(),
        };
        if options.ignore_case {
            normalized = normalized.to_lowercase();
        }
        keys.push(LineKey {
            text: normalized,
            ending: if options.ignore_line_endings {
                Ending::Ignored
            } else {
                ending
            },
        });
        original_indexes.push(index);
    }
    Lines {
        keys,
        original_indexes,
    }
}

/// A nonempty hunk includes ignored lines *between* its changed real lines, but
/// never consumes surrounding ignored lines. Insertion anchors precede the next
/// retained line (or follow the final retained line). This keeps every range
/// ordered and prevents the most common normalized-offset merge corruption.
fn original_range(lines: &Lines, range: Range<usize>) -> Range<usize> {
    if range.is_empty() {
        let point = lines
            .original_indexes
            .get(range.start)
            .copied()
            .unwrap_or_else(|| lines.original_indexes.last().map_or(0, |last| last + 1));
        point..point
    } else {
        lines.original_indexes[range.start]..lines.original_indexes[range.end - 1] + 1
    }
}

pub fn compare_text(
    left: &str,
    right: &str,
    options: &TextOptions,
) -> Result<TextDiffResult, String> {
    let regex = if options.ignore_line_regex.is_empty() {
        None
    } else {
        Some(Regex::new(&options.ignore_line_regex).map_err(|err| err.to_string())?)
    };
    let left = comparison_lines(left, options, regex.as_ref());
    let right = comparison_lines(right, options, regex.as_ref());
    let algorithm = match options.algorithm {
        TextAlgorithm::Myers => Algorithm::Myers,
        TextAlgorithm::Patience => Algorithm::Patience,
        TextAlgorithm::Histogram => Algorithm::Histogram,
    };
    let ops = capture_diff_slices(algorithm, &left.keys, &right.keys);
    let mut result = TextDiffResult::default();
    let mut pending: Option<(Range<usize>, Range<usize>)> = None;

    let flush = |pending: &mut Option<(Range<usize>, Range<usize>)>,
                 result: &mut TextDiffResult| {
        if let Some((old, new)) = pending.take() {
            let modified = old.len().min(new.len());
            result.modified += modified;
            result.removed += old.len() - modified;
            result.added += new.len() - modified;
            let old = original_range(&left, old);
            let new = original_range(&right, new);
            result.hunks.push(TextHunk {
                left_start: old.start,
                left_end: old.end,
                right_start: new.start,
                right_end: new.end,
            });
        }
    };

    for op in ops {
        let (tag, old, new) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            flush(&mut pending, &mut result);
            result.unchanged += old.len();
        } else if let Some((pending_old, pending_new)) = pending.as_mut() {
            pending_old.end = old.end;
            pending_new.end = new.end;
        } else {
            pending = Some((old, new));
        }
    }
    flush(&mut pending, &mut result);
    result.equal = result.hunks.is_empty();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_algorithm_returns_original_ranges() {
        for algorithm in [
            TextAlgorithm::Myers,
            TextAlgorithm::Patience,
            TextAlgorithm::Histogram,
        ] {
            let options = TextOptions {
                algorithm,
                ..TextOptions::default()
            };
            let result = compare_text(
                "one\ntwo\nthree\n",
                "one\nchanged\nthree\nextra\n",
                &options,
            )
            .unwrap();
            assert_eq!(result.modified, 1);
            assert_eq!(result.added, 1);
            assert_eq!(result.removed, 0);
            let mut merged: Vec<_> = "one\ntwo\nthree\n".split('\n').collect();
            let right: Vec<_> = "one\nchanged\nthree\nextra\n".split('\n').collect();
            for hunk in result.hunks.iter().rev() {
                merged.splice(
                    hunk.left_start..hunk.left_end,
                    right[hunk.right_start..hunk.right_end].iter().copied(),
                );
            }
            assert_eq!(merged.join("\n"), "one\nchanged\nthree\nextra\n");
        }
    }

    #[test]
    fn blank_line_filter_does_not_shift_merge_coordinates() {
        let options = TextOptions {
            ignore_blank_lines: true,
            ..TextOptions::default()
        };
        let result =
            compare_text("header\n\nold\n\ntail\n", "header\nnew\ntail\n", &options).unwrap();
        assert_eq!(
            result.hunks,
            vec![TextHunk {
                left_start: 2,
                left_end: 3,
                right_start: 1,
                right_end: 2
            }]
        );
        let result = compare_text("header\n\ntail", "header\ninserted\ntail", &options).unwrap();
        assert_eq!(
            result.hunks,
            vec![TextHunk {
                left_start: 2,
                left_end: 2,
                right_start: 1,
                right_end: 2
            }]
        );
        assert!(compare_text("\n \n", "", &options).unwrap().equal);
    }

    #[test]
    fn prefixes_and_regex_preserve_original_coordinates() {
        let options = TextOptions {
            ignore_line_prefixes: vec!["#".into(), "  ".into()],
            ignore_line_regex: "^DEBUG ".into(),
            ..TextOptions::default()
        };
        let result = compare_text(
            "head\n  # old\nDEBUG a\nold\ntail",
            "head\n# new\nDEBUG b\nnew\ntail",
            &options,
        )
        .unwrap();
        assert_eq!(
            result.hunks,
            vec![TextHunk {
                left_start: 3,
                left_end: 4,
                right_start: 3,
                right_end: 4,
            }]
        );
        assert!(compare_text("# old\nkeep", "# new\nkeep", &options)
            .unwrap()
            .equal);
        assert!(!compare_text("x # old", "x # new", &options).unwrap().equal);
        let invalid = TextOptions {
            ignore_line_regex: "[".into(),
            ..options
        };
        assert!(compare_text("a", "a", &invalid).is_err());
    }

    #[test]
    fn newline_style_and_final_newline_are_distinct_rules() {
        let options = TextOptions::default();
        assert!(compare_text("a\r\nb\r", "a\nb\n", &options).unwrap().equal);
        assert!(!compare_text("a", "a\n", &options).unwrap().equal);
        let strict = TextOptions {
            ignore_line_endings: false,
            ..options
        };
        assert!(!compare_text("a\r\nb", "a\nb", &strict).unwrap().equal);
        assert_eq!(original_lines("a\r\nb\rc\n").len(), 4);
    }

    #[test]
    fn unicode_case_and_space_rules_only_change_comparison_keys() {
        let options = TextOptions {
            ignore_case: true,
            whitespace: Whitespace::All,
            ..TextOptions::default()
        };
        assert!(compare_text(" Ä 你\t好 ", "ä你好", &options).unwrap().equal);
        let options = TextOptions {
            whitespace: Whitespace::Trim,
            ..TextOptions::default()
        };
        assert!(compare_text(" x ", "x", &options).unwrap().equal);
        assert!(!compare_text("a b", "ab", &options).unwrap().equal);
    }
}
