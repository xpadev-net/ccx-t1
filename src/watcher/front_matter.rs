use serde::Deserialize;

/// Parsed YAML front matter from a task.md file.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct TaskFrontMatter {
    pub status: Option<String>,
}

/// Extract and parse YAML front matter delimited by `---` markers.
///
/// Returns a default `TaskFrontMatter` (all fields `None`) when:
/// - the document has no opening `---` delimiter, or
/// - no closing `---` delimiter is found.
///
/// Returns `Err` only when the front matter block is present but contains
/// invalid YAML.
pub fn parse_front_matter(content: &str) -> Result<TaskFrontMatter, serde_yaml::Error> {
    // Allow leading blank lines before the opening delimiter.
    let trimmed = content.trim_start_matches(|c| c == '\r' || c == '\n');

    if !trimmed.starts_with("---") {
        return Ok(TaskFrontMatter::default());
    }

    // The opening --- must be followed by a newline, not more dashes or text.
    let rest = &trimmed[3..];
    let body = if let Some(s) = rest.strip_prefix('\n') {
        s
    } else if let Some(s) = rest.strip_prefix("\r\n") {
        s
    } else {
        return Ok(TaskFrontMatter::default());
    };

    // Find the closing delimiter: \n--- followed by \n, \r\n, or end-of-string.
    if let Some(pos) = body.find("\n---") {
        let after_close = &body[pos + 4..];
        if after_close.is_empty() || after_close.starts_with('\n') || after_close.starts_with("\r\n") {
            return serde_yaml::from_str(&body[..pos]);
        }
    }

    Ok(TaskFrontMatter::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_front_matter_returns_default() {
        let result = parse_front_matter("# My Task\n\nsome content").unwrap();
        assert_eq!(result, TaskFrontMatter::default());
    }

    #[test]
    fn empty_content_returns_default() {
        let result = parse_front_matter("").unwrap();
        assert_eq!(result, TaskFrontMatter::default());
    }

    #[test]
    fn valid_status_is_parsed() {
        let content = "---\nstatus: working\n---\n# My Task\n";
        let result = parse_front_matter(content).unwrap();
        assert_eq!(result.status.as_deref(), Some("working"));
    }

    #[test]
    fn missing_status_field_returns_none() {
        let content = "---\nother_key: value\n---\n# My Task\n";
        let result = parse_front_matter(content).unwrap();
        assert_eq!(result.status, None);
    }

    #[test]
    fn no_closing_delimiter_returns_default() {
        let content = "---\nstatus: assigned\n# no closing marker\n";
        let result = parse_front_matter(content).unwrap();
        assert_eq!(result, TaskFrontMatter::default());
    }

    #[test]
    fn malformed_yaml_returns_error() {
        let content = "---\nstatus: [unclosed\n---\n";
        assert!(parse_front_matter(content).is_err());
    }

    #[test]
    fn leading_blank_lines_are_ignored() {
        let content = "\n\n---\nstatus: pr_open\n---\n";
        let result = parse_front_matter(content).unwrap();
        assert_eq!(result.status.as_deref(), Some("pr_open"));
    }

    #[test]
    fn all_known_status_values_round_trip() {
        for status in [
            "assigned", "working", "pr_open", "gate_check", "review_fixing",
            "merge_ready", "returned", "blocked", "failed", "followup_required", "merged",
        ] {
            let content = format!("---\nstatus: {status}\n---\n");
            let result = parse_front_matter(&content).unwrap();
            assert_eq!(result.status.as_deref(), Some(status), "failed for status={status}");
        }
    }
}
