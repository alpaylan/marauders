use std::path::PathBuf;

use anyhow::{anyhow, bail};

use crate::code::{Code, Span, SpanContent};
use crate::languages::Language;
use crate::variation::{Variant, Variation};
use crate::VariantBody;

#[derive(Debug, Default, Clone)]
struct PreprocessorMetadata {
    variation_name: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug)]
struct LineEntry {
    text: String,
    raw: String,
}

pub(crate) fn looks_like_mutations(input: &str) -> bool {
    input.contains("#if defined(M_") && input.contains("#endif")
}

pub(crate) fn render_preprocessor_code_from_comment(spans: &[Span]) -> anyhow::Result<String> {
    let mut output = String::new();

    for span in spans {
        match &span.content {
            SpanContent::Line(line) => output.push_str(line),
            SpanContent::Variation(variation) => {
                if variation.variants.is_empty() {
                    for line in variation.base.lines() {
                        output.push_str(&line);
                        output.push('\n');
                    }
                    continue;
                }

                let metadata = render_metadata(variation);
                let directive_indent = &variation.indentation;

                for (idx, variant) in variation.variants.iter().enumerate() {
                    if idx == 0 {
                        output.push_str(directive_indent);
                        output.push_str("#if defined(M_");
                        output.push_str(&variant.name);
                        output.push(')');
                        if !metadata.is_empty() {
                            output.push(' ');
                            output.push_str(&metadata);
                        }
                        output.push('\n');
                    } else {
                        output.push_str(directive_indent);
                        output.push_str("#elif defined(M_");
                        output.push_str(&variant.name);
                        output.push_str(")\n");
                    }

                    for line in variant.lines() {
                        output.push_str(&line);
                        output.push('\n');
                    }
                }

                output.push_str(directive_indent);
                output.push_str("#else\n");
                for line in variation.base.lines() {
                    output.push_str(&line);
                    output.push('\n');
                }
                output.push_str(directive_indent);
                output.push_str("#endif\n");
            }
        }
    }

    Ok(output)
}

pub(crate) fn render_comment_code_from_preprocessor(
    language: Language,
    input: &str,
) -> anyhow::Result<String> {
    let spans = parse_preprocessor_code(input)?;
    let code = Code::new(language, spans, PathBuf::new());
    Ok(format!("{}", code))
}

pub(crate) fn parse_preprocessor_code(input: &str) -> anyhow::Result<Vec<Span>> {
    let lines = split_lines(input);
    let mut spans = Vec::new();
    let mut constant = String::new();
    let mut constant_start_line = 1;
    let mut cursor = 0;

    while cursor < lines.len() {
        if parse_if_line(&lines[cursor].text).is_some() {
            if !constant.is_empty() {
                spans.push(Span::constant(constant.clone(), constant_start_line));
                constant.clear();
            }

            let span_line = cursor + 1;
            let (variation, next_cursor) = parse_variation_block(&lines, cursor)?;
            spans.push(Span::variation(variation, span_line));
            cursor = next_cursor;
            constant_start_line = cursor + 1;
            continue;
        }

        if constant.is_empty() {
            constant_start_line = cursor + 1;
        }
        constant.push_str(&lines[cursor].raw);
        cursor += 1;
    }

    if !constant.is_empty() {
        spans.push(Span::constant(constant, constant_start_line));
    }

    Ok(spans)
}

fn parse_variation_block(lines: &[LineEntry], start: usize) -> anyhow::Result<(Variation, usize)> {
    let (indentation, first_variant, metadata) = parse_if_line(&lines[start].text)
        .ok_or_else(|| anyhow!("expected '#if defined(M_...)' at line {}", start + 1))?;

    let mut branches: Vec<(String, Vec<String>)> = Vec::new();
    let mut cursor = start + 1;
    let mut current_variant = first_variant;
    let mut current_lines = Vec::new();

    loop {
        if cursor >= lines.len() {
            bail!("unterminated preprocessor mutation block");
        }

        let text = lines[cursor].text.trim_start();
        if let Some(next_variant) = parse_elif_line(text) {
            branches.push((current_variant, current_lines));
            current_variant = next_variant;
            current_lines = Vec::new();
            cursor += 1;
            continue;
        }
        if is_else_line(text) {
            branches.push((current_variant, current_lines));
            cursor += 1;
            break;
        }
        if is_endif_line(text) {
            bail!("preprocessor mutation block is missing '#else' section");
        }

        current_lines.push(lines[cursor].text.clone());
        cursor += 1;
    }

    let mut base_lines = Vec::new();
    loop {
        if cursor >= lines.len() {
            bail!("unterminated preprocessor mutation block (missing '#endif')");
        }
        let text = lines[cursor].text.trim_start();
        if is_endif_line(text) {
            cursor += 1;
            break;
        }
        if parse_elif_line(text).is_some() || is_else_line(text) {
            bail!("unexpected preprocessor directive '{}' in '#else' branch", text);
        }

        base_lines.push(lines[cursor].text.clone());
        cursor += 1;
    }

    let variants: Vec<Variant> = branches
        .iter()
        .map(|(variant_name, variant_lines)| Variant {
            name: variant_name.clone(),
            body: VariantBody::InactiveMultiLine {
                lines: variant_lines.clone(),
                indentation: infer_indentation(variant_lines, &indentation),
            },
        })
        .collect();

    let variant_names: Vec<String> = variants.iter().map(|variant| variant.name.clone()).collect();
    let variation_name = metadata
        .variation_name
        .clone()
        .or_else(|| infer_variation_name(&variant_names));

    let variation = Variation {
        name: variation_name,
        tags: metadata.tags.clone(),
        base: Variant {
            name: "base".to_string(),
            body: VariantBody::Active {
                lines: base_lines.clone(),
            },
        },
        variants,
        active: 0,
        indentation: indentation.clone(),
    };

    Ok((variation, cursor))
}

fn parse_if_line(line: &str) -> Option<(String, String, PreprocessorMetadata)> {
    let indentation = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let trimmed = line.trim_start();
    let (variant, trailing) = parse_variant_directive(trimmed, "#if")?;
    let metadata = parse_metadata(trailing);
    Some((indentation, variant, metadata))
}

fn parse_elif_line(line: &str) -> Option<String> {
    let (variant, _) = parse_variant_directive(line, "#elif")?;
    Some(variant)
}

fn parse_variant_directive<'a>(line: &'a str, keyword: &str) -> Option<(String, &'a str)> {
    let body = line.strip_prefix(keyword)?.trim_start();
    let body = body.strip_prefix("defined(M_")?;
    let close_idx = body.find(')')?;
    let variant_name = body[..close_idx].trim();
    if variant_name.is_empty() {
        return None;
    }

    let trailing = &body[(close_idx + 1)..];
    Some((variant_name.to_string(), trailing))
}

fn is_else_line(line: &str) -> bool {
    line.starts_with("#else")
}

fn is_endif_line(line: &str) -> bool {
    line.starts_with("#endif")
}

fn parse_metadata(trailing: &str) -> PreprocessorMetadata {
    let marker = "marauders:variation=";
    let Some(marker_idx) = trailing.find(marker) else {
        return PreprocessorMetadata::default();
    };

    let mut metadata = PreprocessorMetadata::default();
    let rest = &trailing[(marker_idx + marker.len())..];
    let (variation_part, tail) = match rest.find(';') {
        Some(idx) => (&rest[..idx], &rest[(idx + 1)..]),
        None => (rest, ""),
    };

    let variation_name = variation_part.trim();
    if !variation_name.is_empty() {
        metadata.variation_name = Some(variation_name.to_string());
    }

    if let Some(tags_idx) = tail.find("tags=") {
        let tags_part = &tail[(tags_idx + "tags=".len())..];
        let tags_part = tags_part
            .split("*/")
            .next()
            .map(str::trim)
            .unwrap_or(tags_part);
        metadata.tags = tags_part
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(|tag| tag.to_string())
            .collect();
    }

    metadata
}

fn render_metadata(variation: &Variation) -> String {
    if variation.name.is_none() && variation.tags.is_empty() {
        return String::new();
    }

    let name = variation.name.as_deref().unwrap_or("");
    let tags = variation.tags.join(",");
    format!("/* marauders:variation={name};tags={tags} */")
}

fn infer_variation_name(variant_names: &[String]) -> Option<String> {
    let first = variant_names.first()?;
    let mut prefix = first.clone();
    for name in variant_names.iter().skip(1) {
        prefix = common_prefix(&prefix, name);
        if prefix.is_empty() {
            break;
        }
    }

    let trimmed = prefix.trim_end_matches('_').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn common_prefix(left: &str, right: &str) -> String {
    let mut output = String::new();
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char != right_char {
            break;
        }
        output.push(left_char);
    }
    output
}

fn infer_indentation(lines: &[String], fallback: &str) -> String {
    lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).collect())
        .unwrap_or_else(|| fallback.to_string())
}

fn split_lines(input: &str) -> Vec<LineEntry> {
    input
        .split_inclusive('\n')
        .map(|raw| LineEntry {
            text: raw.strip_suffix('\n').unwrap_or(raw).to_string(),
            raw: raw.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_to_preprocessor_and_back_roundtrip_structure() {
        let comment = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith, core] */
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
        let preprocessor = render_preprocessor_code_from_comment(&spans).unwrap();
        assert!(preprocessor.contains("#if defined(M_add_1)"));
        assert!(preprocessor.contains("#elif defined(M_add_2)"));
        assert!(preprocessor.contains("#else"));
        assert!(preprocessor.contains("#endif"));
        assert!(preprocessor.contains("marauders:variation=add;tags=arith,core"));

        let roundtrip = render_comment_code_from_preprocessor(Language::Rust, &preprocessor).unwrap();
        let roundtrip_spans = crate::syntax::comment::parse_code(&roundtrip).unwrap();

        assert_eq!(spans.len(), roundtrip_spans.len());
        let variation = match &roundtrip_spans[1].content {
            SpanContent::Variation(variation) => variation,
            _ => panic!("expected variation"),
        };
        assert_eq!(variation.name.as_deref(), Some("add"));
        assert_eq!(
            variation.variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
            vec!["add_1", "add_2"]
        );
        assert_eq!(variation.tags, vec!["arith".to_string(), "core".to_string()]);
    }

    #[test]
    fn test_looks_like_preprocessor_mutations() {
        assert!(looks_like_mutations("#if defined(M_foo)\n#else\n#endif\n"));
        assert!(!looks_like_mutations("fn f() -> i32 { 42 }\n"));
    }
}
