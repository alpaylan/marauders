use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::code::{Span, SpanContent};
use crate::languages::Language;

#[derive(Debug, Clone)]
pub(crate) struct MatchReplaceApplyResult {
    pub(crate) source_path: PathBuf,
    pub(crate) variation_name: Option<String>,
    pub(crate) previous_active: usize,
    pub(crate) new_active: usize,
    pub(crate) matched_scopes: usize,
    pub(crate) updated_scopes: usize,
}

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

pub(crate) fn list_variant_names(input: &str) -> anyhow::Result<Vec<String>> {
    let document = parse_document(input)?;
    Ok(document
        .variations
        .into_iter()
        .flat_map(|variation| variation.variants.into_iter().map(|variant| variant.name))
        .collect())
}

pub(crate) fn set_variant_in_match_replace(
    input: &str,
    variant_name: &str,
) -> anyhow::Result<MatchReplaceApplyResult> {
    apply_variant_in_match_replace(input, variant_name, true)
}

pub(crate) fn unset_variant_in_match_replace(
    input: &str,
    variant_name: &str,
) -> anyhow::Result<MatchReplaceApplyResult> {
    apply_variant_in_match_replace(input, variant_name, false)
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

fn apply_variant_in_match_replace(
    input: &str,
    variant_name: &str,
    set_variant: bool,
) -> anyhow::Result<MatchReplaceApplyResult> {
    let resolved = resolve_variations(parse_document(input)?)?;
    let variant_index = build_variant_index(&resolved);
    let scope_refs = variant_index.get(variant_name).cloned().ok_or_else(|| {
        anyhow!(
            "variant '{}' not found in match-replace document",
            variant_name
        )
    })?;

    let mut by_path: BTreeMap<PathBuf, Vec<ScopeRef>> = BTreeMap::new();
    for scope_ref in scope_refs {
        by_path
            .entry(scope_ref.source_path.clone())
            .or_default()
            .push(scope_ref);
    }

    let mut pending_writes = Vec::new();
    let mut representative: Option<(PathBuf, Option<String>, usize, usize)> = None;
    let mut matched_scopes = 0usize;
    let mut updated_scopes = 0usize;

    for (source_path, mut refs) in by_path {
        let source = std::fs::read_to_string(&source_path).map_err(|e| {
            anyhow!(
                "failed to read source file '{}' referenced by scope: {}",
                source_path.display(),
                e
            )
        })?;
        let (mut lines, trailing_newline) = split_lines_preserving_tail(&source);
        let mut changed_this_file = false;

        // Process from bottom to top so in-memory line offsets remain stable.
        refs.sort_by(|l, r| r.start_line.cmp(&l.start_line));

        for scope_ref in refs {
            matched_scopes += 1;
            let variation = &resolved[scope_ref.variation_idx];

            let mut alternatives = Vec::with_capacity(1 + variation.variant_lines.len());
            alternatives.push(variation.base_lines.clone());
            alternatives.extend(variation.variant_lines.clone());

            let (start, end_exclusive, current_active) = locate_variation_region(
                &lines,
                variation.start_line,
                &alternatives,
            )
            .ok_or_else(|| {
                anyhow!(
                    "unable to locate scope '{}' in '{}' (line {}) for variant '{}'",
                    variation.variation.scope,
                    source_path.display(),
                    variation.start_line,
                    variant_name
                )
            })?;

            let desired_active = if set_variant {
                scope_ref.variant_idx + 1
            } else {
                0
            };

            // `unset --variant X` only resets scopes currently active as X.
            let should_update = if set_variant {
                current_active != desired_active
            } else {
                current_active == scope_ref.variant_idx + 1
            };

            let new_active = if should_update {
                desired_active
            } else {
                current_active
            };
            if representative.is_none() {
                representative = Some((
                    source_path.clone(),
                    variation.variation.name.clone(),
                    current_active,
                    new_active,
                ));
            }

            if should_update {
                let replacement = if set_variant {
                    variation.variant_lines[scope_ref.variant_idx].clone()
                } else {
                    variation.base_lines.clone()
                };
                lines.splice(start..end_exclusive, replacement);
                updated_scopes += 1;
                changed_this_file = true;
            }
        }

        if changed_this_file {
            let mut output = lines.join("\n");
            if trailing_newline {
                output.push('\n');
            }
            pending_writes.push((source_path, output));
        }
    }

    for (path, output) in pending_writes {
        std::fs::write(path, output)?;
    }

    let (source_path, variation_name, previous_active, new_active) = representative.unwrap_or((
        PathBuf::new(),
        None,
        0,
        0,
    ));
    Ok(MatchReplaceApplyResult {
        source_path,
        variation_name,
        previous_active,
        new_active,
        matched_scopes,
        updated_scopes,
    })
}

fn locate_variation_region(
    source_lines: &[String],
    preferred_start_line: usize,
    alternatives: &[Vec<String>],
) -> Option<(usize, usize, usize)> {
    if alternatives.is_empty() {
        return None;
    }

    let preferred_start = preferred_start_line.saturating_sub(1);

    // Fast path: exact match at the recorded scope start line.
    for (alt_idx, alternative) in alternatives.iter().enumerate() {
        if alternative.is_empty() {
            continue;
        }
        if preferred_start + alternative.len() > source_lines.len() {
            continue;
        }
        if source_lines[preferred_start..preferred_start + alternative.len()] == alternative[..] {
            return Some((
                preferred_start,
                preferred_start + alternative.len(),
                alt_idx,
            ));
        }
    }

    // Fallback: find the closest matching candidate in the file.
    let mut best: Option<(usize, usize, usize, usize)> = None;
    for (alt_idx, alternative) in alternatives.iter().enumerate() {
        if alternative.is_empty() || alternative.len() > source_lines.len() {
            continue;
        }
        for start in 0..=source_lines.len() - alternative.len() {
            if source_lines[start..start + alternative.len()] != alternative[..] {
                continue;
            }
            let distance = start.abs_diff(preferred_start);
            match best {
                Some((best_distance, best_start, best_alt_idx, _)) => {
                    if distance < best_distance
                        || (distance == best_distance
                            && (start < best_start
                                || (start == best_start && alt_idx < best_alt_idx)))
                    {
                        best = Some((distance, start, alt_idx, alternative.len()));
                    }
                }
                None => {
                    best = Some((distance, start, alt_idx, alternative.len()));
                }
            }
        }
    }

    if let Some((_, start, alt_idx, len)) = best {
        return Some((start, start + len, alt_idx));
    }

    // If nothing matched and we have empty alternatives, assume insertion point.
    alternatives
        .iter()
        .enumerate()
        .find(|(_, alternative)| alternative.is_empty())
        .map(|(alt_idx, _)| {
            let start = preferred_start.min(source_lines.len());
            (start, start, alt_idx)
        })
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

    block.push(format!(
        "{}{}",
        indentation,
        language.variation_begin(&title)
    ));
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
        _ => bail!(
            "invalid scope '{}': expected path:line or path:line:col",
            scope
        ),
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

#[derive(Debug, Clone)]
struct ResolvedVariation {
    variation: MatchReplaceVariation,
    source_path: PathBuf,
    start_line: usize,
    base_lines: Vec<String>,
    variant_lines: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
struct ScopeRef {
    variation_idx: usize,
    variant_idx: usize,
    source_path: PathBuf,
    start_line: usize,
}

fn resolve_variations(document: MatchReplaceDocument) -> anyhow::Result<Vec<ResolvedVariation>> {
    document
        .variations
        .into_iter()
        .map(|variation| {
            let (scope_path, start_line, end_line) = parse_scope_components(&variation.scope)?;
            validate_scope_order(start_line, end_line)?;
            Ok(ResolvedVariation {
                source_path: PathBuf::from(scope_path),
                start_line,
                base_lines: split_lines_preserving_tail(&variation.pattern).0,
                variant_lines: variation
                    .variants
                    .iter()
                    .map(|variant| split_lines_preserving_tail(&variant.replacement).0)
                    .collect(),
                variation,
            })
        })
        .collect()
}

fn build_variant_index(resolved_variations: &[ResolvedVariation]) -> HashMap<String, Vec<ScopeRef>> {
    let mut index: HashMap<String, Vec<ScopeRef>> = HashMap::new();
    for (variation_idx, variation) in resolved_variations.iter().enumerate() {
        for (variant_idx, variant) in variation.variation.variants.iter().enumerate() {
            index.entry(variant.name.clone()).or_default().push(ScopeRef {
                variation_idx,
                variant_idx,
                source_path: variation.source_path.clone(),
                start_line: variation.start_line,
            });
        }
    }
    index
}

fn split_lines_preserving_tail(input: &str) -> (Vec<String>, bool) {
    if input.is_empty() {
        return (Vec::new(), false);
    }
    let trailing_newline = input.ends_with('\n');
    let mut lines = input
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
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
    use serde_json::json;

    fn unique_temp_path(stem: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        std::env::temp_dir().join(format!("{stem}_{pid}_{nanos}.rs"))
    }

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
        let source_path =
            std::env::temp_dir().join(format!("marauders_match_replace_source_{pid}_{nanos}.rs"));
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

        let (output_path, roundtrip) =
            render_comment_code_from_match_replace(&match_replace).unwrap();
        assert_eq!(output_path, source_path);
        assert!(roundtrip.contains("fn calc(a: i32, b: i32) -> i32 {"));
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(source_path);
    }

    #[test]
    fn test_set_unset_variant_in_match_replace() {
        let comment = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add */
    a + b
    /*|| add_1 */
    /*|
    a - b
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
        let source_path = std::env::temp_dir().join(format!(
            "marauders_match_replace_set_unset_source_{pid}_{nanos}.rs"
        ));
        let scope_path = source_path.to_string_lossy().to_string();

        let match_replace = render_match_replace_code_from_comment(&spans, &scope_path).unwrap();

        // Match-replace conversion keeps the source file as base.
        std::fs::write(
            &source_path,
            r#"fn calc(a: i32, b: i32) -> i32 {
    a + b
}
"#,
        )
        .unwrap();

        let set_result = set_variant_in_match_replace(&match_replace, "add_1").unwrap();
        assert_eq!(set_result.source_path, source_path);
        assert_eq!(set_result.previous_active, 0);
        assert_eq!(set_result.new_active, 1);
        assert_eq!(set_result.matched_scopes, 1);
        assert_eq!(set_result.updated_scopes, 1);
        let after_set = std::fs::read_to_string(&source_path).unwrap();
        assert!(after_set.contains("a - b"));
        assert!(!after_set.contains("a + b"));

        // Setting the same variant again is a no-op but still reports the state.
        let set_again = set_variant_in_match_replace(&match_replace, "add_1").unwrap();
        assert_eq!(set_again.previous_active, 1);
        assert_eq!(set_again.new_active, 1);
        assert_eq!(set_again.updated_scopes, 0);

        let unset_result = unset_variant_in_match_replace(&match_replace, "add_1").unwrap();
        assert_eq!(unset_result.source_path, source_path);
        assert_eq!(unset_result.previous_active, 1);
        assert_eq!(unset_result.new_active, 0);
        assert_eq!(unset_result.matched_scopes, 1);
        assert_eq!(unset_result.updated_scopes, 1);
        let after_unset = std::fs::read_to_string(&source_path).unwrap();
        assert!(after_unset.contains("a + b"));
        assert!(!after_unset.contains("a - b"));

        let _ = std::fs::remove_file(source_path);
    }

    #[test]
    fn test_set_unset_multi_scope_same_file() {
        let source_path = unique_temp_path("marauders_match_replace_multi_scope_same_file");
        std::fs::write(
            &source_path,
            r#"fn calc(a: i32, b: i32) -> i32 {
    let left = a + b;
    let right = a + b;
    left + right
}
"#,
        )
        .unwrap();

        let document = serde_json::to_string_pretty(&json!([
            {
                "name": "left_mut",
                "scope": format!("{}:2", source_path.to_string_lossy()),
                "match": "    let left = a + b;",
                "variants": [{"name": "bug_multi", "replacement": "    let left = a - b;"}]
            },
            {
                "name": "right_mut",
                "scope": format!("{}:3", source_path.to_string_lossy()),
                "match": "    let right = a + b;",
                "variants": [{"name": "bug_multi", "replacement": "    let right = a - b;"}]
            }
        ]))
        .unwrap();

        let set_result = set_variant_in_match_replace(&document, "bug_multi").unwrap();
        assert_eq!(set_result.matched_scopes, 2);
        assert_eq!(set_result.updated_scopes, 2);
        let after_set = std::fs::read_to_string(&source_path).unwrap();
        assert!(after_set.contains("let left = a - b;"));
        assert!(after_set.contains("let right = a - b;"));

        // Idempotent second set.
        let set_again = set_variant_in_match_replace(&document, "bug_multi").unwrap();
        assert_eq!(set_again.matched_scopes, 2);
        assert_eq!(set_again.updated_scopes, 0);
        let after_set_again = std::fs::read_to_string(&source_path).unwrap();
        assert_eq!(after_set_again, after_set);

        let unset_result = unset_variant_in_match_replace(&document, "bug_multi").unwrap();
        assert_eq!(unset_result.matched_scopes, 2);
        assert_eq!(unset_result.updated_scopes, 2);
        let after_unset = std::fs::read_to_string(&source_path).unwrap();
        assert!(after_unset.contains("let left = a + b;"));
        assert!(after_unset.contains("let right = a + b;"));

        // Idempotent second unset.
        let unset_again = unset_variant_in_match_replace(&document, "bug_multi").unwrap();
        assert_eq!(unset_again.matched_scopes, 2);
        assert_eq!(unset_again.updated_scopes, 0);
        let after_unset_again = std::fs::read_to_string(&source_path).unwrap();
        assert_eq!(after_unset_again, after_unset);

        let _ = std::fs::remove_file(source_path);
    }

    #[test]
    fn test_set_unset_multi_scope_cross_file() {
        let source_path_a = unique_temp_path("marauders_match_replace_multi_scope_cross_file_a");
        let source_path_b = unique_temp_path("marauders_match_replace_multi_scope_cross_file_b");
        std::fs::write(
            &source_path_a,
            "fn a() {\n    let x = 10;\n    let y = 20;\n}\n",
        )
        .unwrap();
        std::fs::write(
            &source_path_b,
            "fn b() {\n    let x = 10;\n    let y = 20;\n}\n",
        )
        .unwrap();

        let document = serde_json::to_string_pretty(&json!([
            {
                "name": "mut_a",
                "scope": format!("{}:2", source_path_a.to_string_lossy()),
                "match": "    let x = 10;",
                "variants": [{"name": "bug_cross", "replacement": "    let x = 11;"}]
            },
            {
                "name": "mut_b",
                "scope": format!("{}:2", source_path_b.to_string_lossy()),
                "match": "    let x = 10;",
                "variants": [{"name": "bug_cross", "replacement": "    let x = 11;"}]
            }
        ]))
        .unwrap();

        let set_result = set_variant_in_match_replace(&document, "bug_cross").unwrap();
        assert_eq!(set_result.matched_scopes, 2);
        assert_eq!(set_result.updated_scopes, 2);
        assert!(std::fs::read_to_string(&source_path_a)
            .unwrap()
            .contains("let x = 11;"));
        assert!(std::fs::read_to_string(&source_path_b)
            .unwrap()
            .contains("let x = 11;"));

        let unset_result = unset_variant_in_match_replace(&document, "bug_cross").unwrap();
        assert_eq!(unset_result.matched_scopes, 2);
        assert_eq!(unset_result.updated_scopes, 2);
        assert!(std::fs::read_to_string(&source_path_a)
            .unwrap()
            .contains("let x = 10;"));
        assert!(std::fs::read_to_string(&source_path_b)
            .unwrap()
            .contains("let x = 10;"));

        let _ = std::fs::remove_file(source_path_a);
        let _ = std::fs::remove_file(source_path_b);
    }

    #[test]
    fn test_unknown_variant_and_scope_parse_error() {
        let source_path = unique_temp_path("marauders_match_replace_unknown_variant");
        std::fs::write(&source_path, "fn demo() {\n    let a = 1;\n}\n").unwrap();

        let valid_document = serde_json::to_string_pretty(&json!([
            {
                "scope": format!("{}:2", source_path.to_string_lossy()),
                "match": "    let a = 1;",
                "variants": [{"name": "known", "replacement": "    let a = 2;"}]
            }
        ]))
        .unwrap();

        let unknown = set_variant_in_match_replace(&valid_document, "does_not_exist").unwrap_err();
        assert!(unknown.to_string().contains("does_not_exist"));

        let invalid_scope_document = serde_json::to_string_pretty(&json!([
            {
                "scope": format!("{}:line_two", source_path.to_string_lossy()),
                "match": "    let a = 1;",
                "variants": [{"name": "known", "replacement": "    let a = 2;"}]
            }
        ]))
        .unwrap();

        let parse_err = set_variant_in_match_replace(&invalid_scope_document, "known").unwrap_err();
        let parse_err_text = parse_err.to_string();
        assert!(parse_err_text.contains(&source_path.to_string_lossy().to_string()));
        assert!(parse_err_text.contains("line_two"));

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
