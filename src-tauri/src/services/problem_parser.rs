use serde::Serialize;
use std::collections::{HashMap, HashSet};

const MAX_BUFFER_LENGTH: usize = 24_000;
const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "rs", "py", "go", "vue", "svelte", "css", "scss", "html",
];
const TEST_EXTENSIONS: &[&str] = &[
    "test.ts", "test.tsx", "test.js", "test.jsx", "spec.ts", "spec.tsx", "spec.js", "spec.jsx",
    "test.rs", "test.py", "test.go",
];

#[derive(Debug, Clone, Serialize)]
pub struct ProblemEntry {
    pub id: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub severity: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProblemParseResult {
    pub buffer: String,
    pub problems: Vec<ProblemEntry>,
}

pub fn append_and_parse_terminal_problems(
    previous_buffer: &str,
    chunk: &str,
    terminal_id: &str,
) -> ProblemParseResult {
    let buffer =
        trim_buffer(&strip_ansi(&format!("{}{}", previous_buffer, chunk)).replace("\r\n", "\n"));
    let problems = parse_terminal_problems(&buffer, terminal_id);
    ProblemParseResult { buffer, problems }
}

pub fn parse_terminal_problems(output: &str, terminal_id: &str) -> Vec<ProblemEntry> {
    let output = strip_ansi(output).replace("\r\n", "\n");
    let failed_test_files = find_failed_test_files(&output);
    let mut problems: HashMap<String, ProblemEntry> = HashMap::new();
    let mut locations = HashSet::new();

    for (line_index, line) in output.lines().enumerate() {
        for candidate in candidate_locations(line) {
            if !looks_like_source_file(&candidate.file) {
                continue;
            }
            let file = normalize_file(&candidate.file);
            let location_key = format!("{}:{}:{}", file, candidate.line, candidate.column);
            if locations.contains(&location_key) {
                continue;
            }
            locations.insert(location_key);
            let message = clean_message(
                candidate
                    .message
                    .or_else(|| infer_test_message(&output, line_index))
                    .or_else(|| infer_message(&output, line_index))
                    .unwrap_or_else(|| "Terminal reported a problem".to_string()),
            );
            let severity = if message.to_lowercase().contains("warning")
                || message.to_lowercase().contains("warn")
            {
                "warning"
            } else {
                "error"
            };
            let id = format!(
                "terminal-{}-{}-{}-{}-{}",
                terminal_id, file, candidate.line, candidate.column, message
            );
            problems.insert(
                id.clone(),
                ProblemEntry {
                    id,
                    file,
                    line: candidate.line,
                    column: candidate.column,
                    severity: severity.to_string(),
                    source: "test".to_string(),
                    message,
                },
            );
        }
    }

    for file in failed_test_files {
        if problems.values().any(|problem| problem.file == file) {
            continue;
        }
        let id = format!("terminal-{}-{}-1-1-test-failed", terminal_id, file);
        problems.insert(
            id.clone(),
            ProblemEntry {
                id,
                file,
                line: 1,
                column: 1,
                severity: "error".to_string(),
                source: "test".to_string(),
                message: "Test failed".to_string(),
            },
        );
    }

    let mut values: Vec<ProblemEntry> = problems.into_values().collect();
    values.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
    });
    values
}

#[derive(Debug)]
struct CandidateLocation {
    file: String,
    line: usize,
    column: usize,
    message: Option<String>,
}

fn candidate_locations(line: &str) -> Vec<CandidateLocation> {
    let mut locations = Vec::new();
    let bytes = line.as_bytes();
    for (idx, ch) in line.char_indices() {
        if ch != ':' {
            continue;
        }
        let Some((line_number, after_line)) = parse_number_after(&line[idx + 1..]) else {
            continue;
        };
        let after_line_start = idx + 1 + after_line;
        if bytes.get(after_line_start) != Some(&b':') {
            continue;
        }
        let Some((column, after_column)) = parse_number_after(&line[after_line_start + 1..]) else {
            continue;
        };
        let raw_file = trim_stack_prefix(&line[..idx]).trim();
        let file = extract_file_suffix(raw_file);
        if file.is_empty() {
            continue;
        }
        let message_start = after_line_start + 1 + after_column;
        let message = line
            .get(message_start..)
            .map(|value| value.trim_start_matches([' ', '-', ':']).trim().to_string())
            .filter(|value| !value.is_empty());
        locations.push(CandidateLocation {
            file,
            line: line_number,
            column,
            message,
        });
    }

    if let Some(paren) = line.rfind('(') {
        if let Some(end) = line[paren + 1..].find(')') {
            let inner = &line[paren + 1..paren + 1 + end];
            locations.extend(candidate_locations(inner));
        }
    }

    locations
}

fn parse_number_after(value: &str) -> Option<(usize, usize)> {
    let mut end = 0;
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    value[..end]
        .parse::<usize>()
        .ok()
        .map(|number| (number, end))
}

fn trim_stack_prefix(value: &str) -> &str {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix("at ").unwrap_or(trimmed).trim();
    if let Some(paren) = trimmed.rfind('(') {
        return &trimmed[paren + 1..];
    }
    trimmed
}

fn extract_file_suffix(value: &str) -> String {
    let value = value.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(idx) = value.find("file:///") {
        return value[idx..].to_string();
    }
    if let Some(idx) = value.find("file://") {
        return value[idx..].to_string();
    }
    if let Some(idx) = value.find(|ch: char| ch.is_ascii_alphabetic()) {
        let suffix = &value[idx..];
        if suffix.len() >= 3 && suffix.as_bytes().get(1) == Some(&b':') {
            return suffix.to_string();
        }
    }
    value.to_string()
}

fn looks_like_source_file(file: &str) -> bool {
    let normalized = normalize_file(file);
    let lower = normalized.to_lowercase();
    SOURCE_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(&format!(".{}", ext)))
}

fn find_failed_test_files(output: &str) -> Vec<String> {
    let mut files = HashSet::new();
    for line in output.lines() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("FAIL")
            || trimmed.starts_with("FAILED")
            || trimmed.starts_with('✕')
            || trimmed.starts_with('×')
            || trimmed.starts_with('❯'))
        {
            continue;
        }
        for word in trimmed.split_whitespace().skip(1) {
            let cleaned = word.trim_matches(|ch: char| ch == ':' || ch == ')' || ch == '(');
            let lower = cleaned.to_lowercase();
            if TEST_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
                files.insert(normalize_file(cleaned));
            }
        }
    }
    let mut values: Vec<String> = files.into_iter().collect();
    values.sort();
    values
}

fn normalize_file(file: &str) -> String {
    percent_decode(file.trim())
        .trim_start_matches("file:///")
        .trim_start_matches("file://")
        .trim_start_matches('/')
        .replace('\\', "/")
}

fn percent_decode(value: &str) -> String {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' && idx + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[idx + 1..idx + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    output.push(value as char);
                    idx += 3;
                    continue;
                }
            }
        }
        output.push(bytes[idx] as char);
        idx += 1;
    }
    output
}

fn clean_message(message: String) -> String {
    let mut value = message.trim().trim_start_matches('>').trim().to_string();
    for prefix in ["error:", "warning:", "AssertionError:"] {
        if value.to_lowercase().starts_with(&prefix.to_lowercase()) {
            value = value[prefix.len()..].trim().to_string();
        }
    }
    if value.to_lowercase().starts_with("error ts") {
        if let Some(idx) = value.find(':') {
            value = value[idx + 1..].trim().to_string();
        }
    }
    value
}

fn infer_message(output: &str, line_index: usize) -> Option<String> {
    output
        .lines()
        .skip(line_index + 1)
        .take(4)
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn infer_test_message(output: &str, line_index: usize) -> Option<String> {
    let lines: Vec<&str> = output.lines().collect();
    lines
        .iter()
        .take(line_index)
        .rev()
        .take(8)
        .map(|line| line.trim())
        .find(|line| {
            let lower = line.to_lowercase();
            lower.contains("assertionerror")
                || lower.contains("error:")
                || lower.contains("expected")
                || lower.contains("received")
                || lower.contains("fail")
                || lower.contains("failed")
        })
        .map(str::to_string)
}

fn strip_ansi(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn trim_buffer(buffer: &str) -> String {
    if buffer.len() <= MAX_BUFFER_LENGTH {
        buffer.to_string()
    } else {
        buffer[buffer.len() - MAX_BUFFER_LENGTH..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_esm_file_uri_stack_trace() {
        let output = [
            "ReferenceError: xxxxx is not defined",
            "    at file:///D:/work/openclaw-workspace/chrome-mcp-client/test.js:18:1",
            "    at ModuleJob.run (node:internal/modules/esm/module_job:437:25)",
        ]
        .join("\n");

        let problems = parse_terminal_problems(&output, "test");

        assert_eq!(problems.len(), 1);
        assert_eq!(
            problems[0].file,
            "D:/work/openclaw-workspace/chrome-mcp-client/test.js"
        );
        assert_eq!(problems[0].line, 18);
        assert_eq!(problems[0].column, 1);
        assert_eq!(problems[0].severity, "error");
        assert_eq!(problems[0].source, "test");
    }

    #[test]
    fn parses_windows_colon_location() {
        let output = "src\\main.rs:12:4: error: expected expression";

        let problems = parse_terminal_problems(output, "task");

        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].file, "src/main.rs");
        assert_eq!(problems[0].message, "expected expression");
    }

    #[test]
    fn keeps_enough_buffer_to_parse_split_output() {
        let first = append_and_parse_terminal_problems(
            "",
            "ReferenceError: xxxxx is not defined\n    at file:///D:/repo/test.js",
            "main",
        );
        let second = append_and_parse_terminal_problems(&first.buffer, ":9:3\n", "main");

        assert_eq!(second.problems.len(), 1);
        assert_eq!(second.problems[0].file, "D:/repo/test.js");
        assert_eq!(second.problems[0].line, 9);
        assert_eq!(second.problems[0].column, 3);
    }
}
