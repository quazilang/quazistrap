// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Parseable approximations of a document that is being edited.
//!
//! While the user types, the document usually fails to parse. Semantic
//! features still need a report whose spans match the current text, so the
//! server analyzes a copy in which the broken region is blanked: every
//! character except line breaks becomes a space. Character offsets and line
//! numbers therefore stay identical to the edited text, and every declaration
//! outside the blanked region keeps its exact span.

/// Candidate texts for a document whose parse failed on 1-based `error_line`,
/// from the smallest blanked region to the largest: the error line, the
/// previous non-blank line (an unfinished line is often reported at the next
/// token, such as a closing brace), both, and the whole enclosing top-level
/// item.
pub fn candidates(source: &str, error_line: usize) -> Vec<String> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let error = error_line.saturating_sub(1).min(lines.len() - 1);
    let mut regions = vec![(error, error + 1)];
    if let Some(previous) = (0..error).rev().find(|&line| !lines[line].trim().is_empty()) {
        regions.push((previous, previous + 1));
        regions.push((previous, error + 1));
    }
    let (start, end) = enclosing_item(&lines, error);
    if !regions.contains(&(start, end)) {
        regions.push((start, end));
    }
    regions
        .into_iter()
        .map(|(start, end)| blank_lines(&lines, start, end))
        .collect()
}

/// Lines `[start, end)` of the top-level item around `line`: items begin at a
/// line whose first character is neither whitespace, `}` nor a comment.
fn enclosing_item(lines: &[&str], line: usize) -> (usize, usize) {
    let starts_item = |index: usize| {
        let text = lines[index];
        let first = text.chars().next();
        first.is_some_and(|ch| !ch.is_whitespace() && ch != '}') && !text.starts_with("//")
    };
    let mut start = (0..=line).rev().find(|&index| starts_item(index)).unwrap_or(0);
    // Attributes and doc comments directly above belong to the same item.
    while start > 0 {
        let above = lines[start - 1].trim_start();
        if above.starts_with('@') || above.starts_with("///") {
            start -= 1;
        } else {
            break;
        }
    }
    let end = (line + 1..lines.len())
        .find(|&index| starts_item(index) && !lines[index].starts_with('@'))
        .map(|next| {
            // Keep the next item's own attributes.
            let mut end = next;
            while end > line + 1 && lines[end - 1].trim_start().starts_with('@') {
                end -= 1;
            }
            end
        })
        .unwrap_or(lines.len());
    (start, end)
}

fn blank_lines(lines: &[&str], start: usize, end: usize) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if (start..end).contains(&index) {
                line.chars()
                    .map(|ch| if ch == '\n' || ch == '\r' { ch } else { ' ' })
                    .collect()
            } else {
                (*line).to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::candidates;

    #[test]
    fn blanks_keep_offsets_and_line_breaks() {
        let source = "fn a() void {\n    x.\n}\r\nfn b() void {}\n";
        let options = candidates(source, 2);
        assert_eq!(options[0], "fn a() void {\n      \n}\r\nfn b() void {}\n");
        assert_eq!(options[1], "             \n    x.\n}\r\nfn b() void {}\n");
        assert_eq!(options[2], "             \n      \n}\r\nfn b() void {}\n");
        assert_eq!(options[3], "             \n      \n \r\nfn b() void {}\n");
        for option in &options {
            assert_eq!(option.chars().count(), source.chars().count());
        }
    }

    #[test]
    fn enclosing_item_includes_attributes_and_stops_before_the_next_ones() {
        let source = "@test\nfn a() void {\n    let\n}\n@inline\nfn b() void {}\n";
        let item = candidates(source, 3).pop().expect("item candidate");
        assert_eq!(item, "     \n             \n       \n \n@inline\nfn b() void {}\n");
    }

    #[test]
    fn non_ascii_text_is_blanked_per_character() {
        let source = "fn a() void {\n    \"λ🙂\".\n}\n";
        let line = &candidates(source, 2)[0];
        assert_eq!(line.chars().count(), source.chars().count());
        assert!(line.lines().nth(1).unwrap().trim().is_empty());
    }
}
