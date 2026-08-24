//! A small YAML-frontmatter reader.
//!
//! Deliberately not a YAML parser. We need three things — `title`, `tags`, and
//! where the body starts — and a full parser would invite storing structures the
//! rest of the system cannot round-trip. Anything richer stays in the file,
//! untouched, which is the whole point of a human-editable vault.

pub struct Parsed<'a> {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub body: &'a str,
}

pub fn parse(text: &str) -> Parsed<'_> {
    let mut out = Parsed {
        title: None,
        tags: Vec::new(),
        body: text,
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return out;
    };
    let Some(end) = rest.find("\n---") else {
        return out;
    };
    let (front, after) = rest.split_at(end);
    out.body = after
        .trim_start_matches('\n')
        .trim_start_matches("---")
        .trim_start_matches('\n');

    for line in front.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        match key.trim() {
            "title" if !value.is_empty() => out.title = Some(value.to_string()),
            "tags" => out.tags = parse_tags(value),
            _ => {}
        }
    }
    out
}

/// Accepts `[a, b]` and `a, b`. The YAML block-list form is not supported and
/// is left to the file rather than half-parsed.
fn parse_tags(value: &str) -> Vec<String> {
    value
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|t| t.trim().trim_matches(['"', '\'']).to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

pub fn first_heading(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Extract `[[target]]` and `[[target|alias]]` link targets, in order, deduped.
pub fn wikilinks(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("[[") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("]]") else { break };
        let target = rest[..end]
            .split('|')
            .next()
            .unwrap_or_default()
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if !target.is_empty() && !out.contains(&target) {
            out.push(target);
        }
        rest = &rest[end + 2..];
    }
    out
}

pub fn render(title: Option<&str>, tags: &[String]) -> String {
    if title.is_none() && tags.is_empty() {
        return String::new();
    }
    let mut out = String::from("---\n");
    if let Some(title) = title {
        out.push_str(&format!("title: \"{}\"\n", title.replace('"', "'")));
    }
    if !tags.is_empty() {
        out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    out.push_str("---\n\n");
    out
}
