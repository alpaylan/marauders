use std::path::PathBuf;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::code::{Span, SpanContent};
use crate::languages::Language;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatchReplaceVariation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    indentation: Option<String>,
    scope: String,
    #[serde(rename = "match")]
    pattern: String,
    variants: Vec<MatchReplaceVariant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatchReplaceVariant {
    name: String,
    replacement: String,
}

pub(crate) fn looks_like_mutations(input: &str) -> bool {
    parse_document(input).is_ok()
}

pub(crate) fn render_match_replace_code_from_comment(
    spans: &[Span],
    source_path: &str,
) -> anyhow::Result<String> {
    let mut variations = Vec::new();
    let mut current_line = 1usize;

    for span in spans {
        match &span.content {
            SpanContent::Line(line) => {
                current_line += count_lines(line);
            }
            SpanContent::Variation(variation) => {
                let start_line = current_line;
                let base_lines = variation.base.lines();
                let end_line = if base_lines.is_empty() {
                    start_line.saturating_sub(1)
                } else {
                    start_line + base_lines.len() - 1
                };
                current_line += base_lines.len();

                variations.push(MatchReplaceVariation {
                    name: variation.name.clone(),
                    tags: variation.tags.clone(),
                    indentation: if variation.indentation.is_empty() {
                        None
                    } else {
                        Some(variation.indentation.clone())
                    },
                    scope: format_scope(source_path, start_line, end_line),
                    pattern: base_lines.join("\n"),
                    variants: variation
                        .variants
                        .iter()
                        .map(|variant| MatchReplaceVariant {
                            name: variant.name.clone(),
                            replacement: variant.lines().join("\n"),
                        })
                        .collect(),
                });
            }
        }
    }

    if variations.len() == 1 {
        serde_json::to_string_pretty(&variations[0]).map_err(|e| anyhow!(e))
    } else {
        serde_json::to_string_pretty(&variations).map_err(|e| anyhow!(e))
    }
}

pub(crate) fn render_comment_code_from_match_replace(
    input: &str,
) -> anyhow::Result<(PathBuf, String)> {
    let mut resolved_variations = parse_document(input)?
        .variations
        .into_iter()
        .map(|variation| {
            let (scope_path, start_line, end_line) = parse_scope_components(&variation.scope)?;
            Ok((variation, scope_path, start_line, end_line))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    if resolved_variations.is_empty() {
        bail!("empty match-replace document");
    }

    let source_path = PathBuf::from(resolved_variations[0].1.clone());
    for (_, scope_path, _, _) in &resolved_variations {
        if scope_path != &resolved_variations[0].1 {
            bail!(
                "multiple source files in one match-replace document are not supported: '{}' and '{}'",
                resolved_variations[0].1,
                scope_path
            );
        }
    }

    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("rs");
    let language = Language::extension_to_language(extension, &vec![]).unwrap_or(Language::Rust);
    let source = std::fs::read_to_string(&source_path).map_err(|e| {
        anyhow!(
            "failed to read source file '{}' referenced by scope: {}",
            source_path.display(),
            e
        )
    })?;
    let (mut lines, trailing_newline) = split_lines_preserving_tail(&source);

    resolved_variations.sort_by(|l, r| r.2.cmp(&l.2).then(r.3.cmp(&l.3)));

    for (variation, _scope_path, start_line, end_line) in resolved_variations {
        validate_range(start_line, end_line, lines.len())?;
        let start = start_line - 1;
        let end_exclusive = end_line;
        let base_fragment = lines[start..end_exclusive].to_vec();
        let variants = variation
            .variants
            .iter()
            .map(|variant| {
                (
                    variant.name.as_str(),
                    split_lines_preserving_tail(&variant.replacement).0,
                )
            })
            .collect::<Vec<_>>();
        let indent = variation
            .indentation
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| infer_indentation(&base_fragment));
        let block = render_comment_variation_block(
            &language,
            variation.name.as_deref(),
            &variation.tags,
            &indent,
            &base_fragment,
            &variants
                .iter()
                .map(|(name, lines)| (*name, lines.as_slice()))
                .collect::<Vec<_>>(),
        );
        lines.splice(start..end_exclusive, block);
    }

    let mut output = lines.join("\n");
    if trailing_newline {
        output.push('\n');
    }

    Ok((source_path, output))
}

fn parse_document(input: &str) -> anyhow::Result<MatchReplaceDocument> {
    #[derive(Debug, Clone, Deserialize)]
    #[serde(untagged)]
    enum Document {
        One(MatchReplaceVariation),
        Many(Vec<MatchReplaceVariation>),
    }

    let doc = serde_json::from_str::<Document>(input).map_err(|e| anyhow!(e))?;
    let variations = match doc {
        Document::One(variation) => vec![variation],
        Document::Many(variations) => variations,
    };

    if variations.is_empty() {
        bail!("empty match-replace document");
    }

    Ok(MatchReplaceDocument { variations })
}

fn render_comment_variation_block(
    language: &Language,
    name: Option<&str>,
    tags: &[String],
    indentation: &str,
    base_lines: &[String],
    variants: &[(&str, &[String])],
) -> Vec<String> {
    let mut block = Vec::new();
    let title = render_variation_title(name, tags);

    block.push(format!("{}{}", indentation, language.variation_begin(&title)));
    block.extend_from_slice(base_lines);
    for (variant_name, replacement_lines) in variants {
        block.push(format!(
            "{}{} {} {}",
            indentation,
            language.variant_header_begin(),
            variant_name,
            language.variant_header_end()
        ));
        block.push(format!("{}{}", indentation, language.variant_body_begin()));
        block.extend(replacement_lines.iter().cloned());
        block.push(format!("{}{}", indentation, language.variant_body_end()));
    }
    block.push(format!("{}{}", indentation, language.variation_end()));
    block
}

fn render_variation_title(name: Option<&str>, tags: &[String]) -> String {
    let mut title = String::new();
    if let Some(name) = name {
        title.push_str(name);
        title.push(' ');
    }
    if !tags.is_empty() {
        title.push('[');
        title.push_str(&tags.join(", "));
        title.push_str("] ");
    }
    title
}

fn format_scope(path: &str, start_line: usize, end_line: usize) -> String {
    if start_line == end_line {
        format!("{path}:{start_line}")
    } else {
        format!("{path}:{start_line}-{end_line}")
    }
}

fn parse_scope_components(scope: &str) -> anyhow::Result<(String, usize, usize)> {
    let parts = scope.rsplitn(3, ':').collect::<Vec<_>>();
    match parts.as_slice() {
        [line_or_range, path] => {
            if let Some((start, end)) = line_or_range.split_once('-') {
                Ok((
                    (*path).to_string(),
                    parse_scope_number(start, scope)?,
                    parse_scope_number(end, scope)?,
                ))
            } else {
                let line = parse_scope_number(line_or_range, scope)?;
                Ok(((*path).to_string(), line, line))
            }
        }
        [col_or_col_range, line, path] => {
            parse_scope_col(col_or_col_range, scope)?;
            let line = parse_scope_number(line, scope)?;
            Ok(((*path).to_string(), line, line))
        }
        _ => bail!("invalid scope '{}': expected path:line or path:line:col", scope),
    }
}

#[cfg(test)]
fn parse_scope_range(scope: &str) -> anyhow::Result<(usize, usize)> {
    let (_, start, end) = parse_scope_components(scope)?;
    Ok((start, end))
}

fn parse_scope_number(value: &str, scope: &str) -> anyhow::Result<usize> {
    let value = value.trim();
    if value.is_empty() {
        bail!("invalid scope '{}': empty numeric component", scope);
    }
    value
        .parse()
        .map_err(|e| anyhow!("invalid scope '{}': {}", scope, e))
}

fn parse_scope_col(value: &str, scope: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty() {
        bail!("invalid scope '{}': empty column component", scope);
    }
    if let Some((start, end)) = value.split_once('-') {
        let _ = parse_scope_number(start, scope)?;
        let _ = parse_scope_number(end, scope)?;
        return Ok(());
    }
    let _ = parse_scope_number(value, scope)?;
    Ok(())
}

fn validate_scope_order(start_line: usize, end_line: usize) -> anyhow::Result<()> {
    if start_line == 0 {
        bail!("invalid start_line=0 in match-replace document");
    }
    if end_line < start_line.saturating_sub(1) {
        bail!(
            "invalid range {}..{} in match-replace document",
            start_line,
            end_line
        );
    }
    Ok(())
}

fn validate_range(start_line: usize, end_line: usize, line_count: usize) -> anyhow::Result<()> {
    validate_scope_order(start_line, end_line)?;
    if end_line > line_count {
        bail!(
            "invalid range {}..{} for source with {} lines",
            start_line,
            end_line,
            line_count
        );
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct MatchReplaceDocument {
    variations: Vec<MatchReplaceVariation>,
}

fn split_lines_preserving_tail(input: &str) -> (Vec<String>, bool) {
    if input.is_empty() {
        return (Vec::new(), false);
    }
    let trailing_newline = input.ends_with('\n');
    let mut lines = input.split('\n').map(|line| line.to_string()).collect::<Vec<_>>();
    if trailing_newline {
        lines.pop();
    }
    (lines, trailing_newline)
}

fn infer_indentation(lines: &[String]) -> String {
    lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).collect())
        .unwrap_or_default()
}

fn count_lines(input: &str) -> usize {
    if input.is_empty() {
        return 0;
    }
    let newlines = input.matches('\n').count();
    if input.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_match_replace_comment_roundtrip() {
        let comment = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /*|| add_2 */
    /*|
    a * b
    */
    /* |*/
}
"#;

        let spans = crate::syntax::comment::parse_code(comment).unwrap();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let source_path = std::env::temp_dir().join(format!("marauders_match_replace_source_{pid}_{nanos}.rs"));
        let scope_path = source_path.to_string_lossy().to_string();

        let match_replace = render_match_replace_code_from_comment(&spans, &scope_path).unwrap();
        assert!(!match_replace.contains("\"format\""));
        assert!(!match_replace.contains("\"base\""));
        assert!(match_replace.contains("\"replacement\": \"    a - b\""));
        assert!(match_replace.contains(&format!("\"scope\": \"{}:3\"", scope_path)));
        assert!(!match_replace.contains("\"location\""));
        assert!(match_replace.contains("\"match\": \"    a + b\""));

        let mut base = String::new();
        for span in &spans {
            match &span.content {
                SpanContent::Line(line) => base.push_str(line),
                SpanContent::Variation(variation) => {
                    for line in variation.base.lines() {
                        base.push_str(&line);
                        base.push('\n');
                    }
                }
            }
        }
        std::fs::write(&source_path, base).unwrap();

        let (output_path, roundtrip) = render_comment_code_from_match_replace(&match_replace).unwrap();
        assert_eq!(output_path, source_path);
        assert!(roundtrip.contains("fn calc(a: i32, b: i32) -> i32 {"));
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(source_path);
    }

    #[test]
    fn test_parse_scope_range_forms() {
        assert_eq!(parse_scope_range("file.rs:7").unwrap(), (7, 7));
        assert_eq!(parse_scope_range("file.rs:7-9").unwrap(), (7, 9));
        assert_eq!(parse_scope_range("file.rs:7:12").unwrap(), (7, 7));
        assert_eq!(parse_scope_range("file.rs:7:12-20").unwrap(), (7, 7));
    }
}
