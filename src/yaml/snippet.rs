//! Port of js-yaml `snippet.js` (error context shown in exception messages).

fn get_line(
    buffer: &[u16],
    mut line_start: isize,
    line_end: Option<isize>,
    position: isize,
    max_line_length: isize,
) -> (String, isize) {
    let mut head = "";
    let mut tail = "";
    let max_half_length = (max_line_length as f64 / 2.0).floor() as isize - 1;
    if position - line_start > max_half_length {
        head = " ... ";
        line_start = position - max_half_length + head.len() as isize;
    }
    let mut line_end = line_end;
    if let Some(end) = line_end
        && end - position > max_half_length
    {
        tail = " ...";
        line_end = Some(position + max_half_length - tail.len() as isize);
    }
    let len = buffer.len() as isize;
    let from = line_start.clamp(0, len) as usize;
    let to = line_end.unwrap_or(len).clamp(0, len) as usize;
    let slice = if from < to { String::from_utf16_lossy(&buffer[from..to]) } else { String::new() };
    let text = format!("{}{}{}", head, slice.replace('\t', "→"), tail);
    (text, position - line_start + head.len() as isize)
}

fn pad_start(s: &str, max: usize) -> String {
    let len = s.chars().count();
    format!("{}{}", " ".repeat(max.saturating_sub(len)), s)
}

/// `makeSnippet(mark)` with default options (maxLength 79, indent 1, 3 lines
/// before, 2 after). Returns None for an empty buffer.
pub fn make_snippet(buffer: &[u16], position: usize, line: usize, _column: isize) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }
    let max_length: isize = 79;
    let indent: usize = 1;
    let lines_before: isize = 3;
    let lines_after: isize = 2;

    let mut line_starts: Vec<isize> = vec![0];
    let mut line_ends: Vec<isize> = Vec::new();
    let mut found_line_no: isize = -1;
    let position = position as isize;
    let mut i = 0;
    while i < buffer.len() {
        let c = buffer[i];
        let match_len = if c == 0x0D && buffer.get(i + 1) == Some(&0x0A) {
            2
        } else if c == 0x0A || c == 0x0D || c == 0 {
            1
        } else {
            0
        };
        if match_len > 0 {
            line_ends.push(i as isize);
            line_starts.push((i + match_len) as isize);
            if position <= i as isize && found_line_no < 0 {
                found_line_no = line_starts.len() as isize - 2;
            }
            i += match_len;
        } else {
            i += 1;
        }
    }
    if found_line_no < 0 {
        found_line_no = line_starts.len() as isize - 1;
    }

    let line_no_length = (line as isize + lines_after).min(line_ends.len() as isize).to_string().len();
    let max_line_length = max_length - (indent as isize + line_no_length as isize + 3);
    let line_end_at = |idx: isize| -> Option<isize> { line_ends.get(idx as usize).copied() };

    let mut result = String::new();
    for i in 1..=lines_before {
        if found_line_no - i < 0 {
            break;
        }
        let idx = found_line_no - i;
        let (text, _) = get_line(
            buffer,
            line_starts[idx as usize],
            line_end_at(idx),
            position - (line_starts[found_line_no as usize] - line_starts[idx as usize]),
            max_line_length,
        );
        result = format!(
            "{}{} | {}\n{}",
            " ".repeat(indent),
            pad_start(&(line as isize - i + 1).to_string(), line_no_length),
            text,
            result
        );
    }

    let (text, pos) =
        get_line(buffer, line_starts[found_line_no as usize], line_end_at(found_line_no), position, max_line_length);
    result.push_str(&format!(
        "{}{} | {}\n",
        " ".repeat(indent),
        pad_start(&(line + 1).to_string(), line_no_length),
        text
    ));
    result.push_str(&"-".repeat((indent as isize + line_no_length as isize + 3 + pos).max(0) as usize));
    result.push_str("^\n");

    for i in 1..=lines_after {
        let idx = found_line_no + i;
        if idx >= line_ends.len() as isize {
            break;
        }
        let (text, _) = get_line(
            buffer,
            line_starts[idx as usize],
            line_end_at(idx),
            position - (line_starts[found_line_no as usize] - line_starts[idx as usize]),
            max_line_length,
        );
        result.push_str(&format!(
            "{}{} | {}\n",
            " ".repeat(indent),
            pad_start(&(line as isize + i + 1).to_string(), line_no_length),
            text
        ));
    }
    if result.ends_with('\n') {
        result.pop();
    }
    Some(result)
}
