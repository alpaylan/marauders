use std::collections::HashMap;
use std::ops::Range;

use pest::Parser as _;
use pest_derive::Parser;
use quote::ToTokens;
use syn::parse_quote;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::code::Span;
use crate::variation::{Variant, Variation};
use crate::VariantBody;

const RUST_MUTATION_ENV_PREFIX: &str = "M_";

/// Functional Mutations are AST-level mutations that reduce the requirement for multiple-compilations.
/// This module implements functional mutations for a toy-lisp.

#[derive(Debug, Clone, PartialEq)]
enum AST {
    // (define symbol expression)
    Define(String, Box<AST>),
    // (lambda (args) body)
    Lambda(Vec<String>, Box<AST>),
    // (if condition then else)
    If(Box<AST>, Box<AST>, Box<AST>),
    // (begin expr1 expr2 ... exprN)
    Begin(Vec<AST>),
    // Atoms
    Symbol(String),
    Number(i64),
    String(String),
    Boolean(bool),
    // (f expr1 ... exprN)
    Call(String, Vec<AST>),
    // (expr1 expr2 ... exprN)
    Apply(Vec<AST>),
    // (mutate expr1 expr2 ... exprN)
    Mutate(Option<String>, Vec<(String, AST)>),
}

#[derive(Debug, Clone, PartialEq)]
enum Builtin {
    Sum,
    Minus,
    Mult,
    And,
    Or,
    Not,
    Eq,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Number(i64),
    Boolean(bool),
    String(String),
    Function(Vec<String>, Box<AST>),
    Builtin(Builtin),
}

#[derive(Parser)]
#[grammar = "syntax/functional.pest"]
struct LispParser;

pub(crate) fn looks_like_rust_mutations(input: &str) -> bool {
    input.contains(r#"std::env::var("M_"#) || input.contains(r#"env::var("M_"#)
}

pub(crate) fn parse_rust_variations(input: &str) -> Vec<Span> {
    parse_rust_variations_ast(input)
}

pub(crate) fn import_rust_mutants_from_files(
    base_source: &str,
    mutant_sources: &[String],
    name_prefix: &str,
) -> anyhow::Result<String> {
    syn::parse_file(base_source)
        .map_err(|err| anyhow::anyhow!("base Rust source is not parseable: {err}"))?;

    if mutant_sources.is_empty() {
        return Err(anyhow::anyhow!(
            "expected at least one mutant source for import"
        ));
    }

    let base_lines = base_source
        .lines()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let trailing_newline = base_source.ends_with('\n');
    if base_lines.is_empty() {
        return Err(anyhow::anyhow!("base source is empty"));
    }

    let mut imported = Vec::<ImportedVariation>::new();
    for (mutant_idx, mutant_source) in mutant_sources.iter().enumerate() {
        syn::parse_file(mutant_source).map_err(|err| {
            anyhow::anyhow!(
                "mutant source {} is not parseable Rust: {}",
                mutant_idx + 1,
                err
            )
        })?;
        let mutant_lines = mutant_source
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let hunks = line_diff_hunks(&base_lines, &mutant_lines);
        if hunks.is_empty() {
            continue;
        }

        for (hunk_idx, hunk) in hunks.into_iter().enumerate() {
            if hunk.base_range.start == hunk.base_range.end {
                return Err(anyhow::anyhow!(
                    "line insertion hunks are not supported (mutant {}, hunk {})",
                    mutant_idx + 1,
                    hunk_idx + 1
                ));
            }
            if hunk.mutant_lines.is_empty() {
                return Err(anyhow::anyhow!(
                    "line deletion hunks are not supported (mutant {}, hunk {})",
                    mutant_idx + 1,
                    hunk_idx + 1
                ));
            }

            let base_chunk = base_lines[hunk.base_range.clone()].to_vec();
            if base_chunk == hunk.mutant_lines {
                continue;
            }

            if let Some(existing) = imported
                .iter_mut()
                .find(|entry| entry.base_range == hunk.base_range && entry.base_lines == base_chunk)
            {
                if !existing.variant_lines.contains(&hunk.mutant_lines) {
                    existing.variant_lines.push(hunk.mutant_lines);
                }
                continue;
            }

            imported.push(ImportedVariation {
                base_range: hunk.base_range,
                base_lines: base_chunk,
                variant_lines: vec![hunk.mutant_lines],
            });
        }
    }

    if imported.is_empty() {
        return Err(anyhow::anyhow!(
            "no line-level differences were found against mutant sources"
        ));
    }

    imported.sort_by_key(|entry| entry.base_range.start);
    for pair in imported.windows(2) {
        if pair[0].base_range.end > pair[1].base_range.start {
            return Err(anyhow::anyhow!(
                "overlapping line hunks were produced by external mutants"
            ));
        }
    }

    let mut rendered = Vec::new();
    let mut cursor = 0usize;

    for (variation_idx, entry) in imported.iter().enumerate() {
        while cursor < entry.base_range.start {
            rendered.push(base_lines[cursor].clone());
            cursor += 1;
        }

        let variation_name = format!("{name_prefix}_{}", variation_idx + 1);
        let indentation = infer_import_indentation(&entry.base_lines);
        rendered.push(format!("{indentation}/*| {variation_name} */"));
        rendered.extend(entry.base_lines.clone());

        for (variant_idx, variant_lines) in entry.variant_lines.iter().enumerate() {
            let variant_name = format!("{variation_name}_{}", variant_idx + 1);
            rendered.push(format!("{indentation}/*|| {variant_name} */"));
            rendered.push(format!("{indentation}/*|"));
            rendered.extend(variant_lines.clone());
            rendered.push(format!("{indentation}*/"));
        }

        rendered.push(format!("{indentation}/* |*/"));
        cursor = entry.base_range.end;
    }

    while cursor < base_lines.len() {
        rendered.push(base_lines[cursor].clone());
        cursor += 1;
    }

    let mut out = rendered.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    Ok(out)
}

#[derive(Clone, Debug)]
struct LineHunk {
    base_range: Range<usize>,
    mutant_lines: Vec<String>,
}

#[derive(Clone, Debug)]
struct ImportedVariation {
    base_range: Range<usize>,
    base_lines: Vec<String>,
    variant_lines: Vec<Vec<String>>,
}

fn line_diff_hunks(base_lines: &[String], mutant_lines: &[String]) -> Vec<LineHunk> {
    let matches = lcs_line_matches(base_lines, mutant_lines);
    let mut out = Vec::new();
    let mut base_cursor = 0usize;
    let mut mutant_cursor = 0usize;

    for (base_idx, mutant_idx) in matches {
        if base_cursor < base_idx || mutant_cursor < mutant_idx {
            out.push(LineHunk {
                base_range: base_cursor..base_idx,
                mutant_lines: mutant_lines[mutant_cursor..mutant_idx].to_vec(),
            });
        }
        base_cursor = base_idx + 1;
        mutant_cursor = mutant_idx + 1;
    }

    if base_cursor < base_lines.len() || mutant_cursor < mutant_lines.len() {
        out.push(LineHunk {
            base_range: base_cursor..base_lines.len(),
            mutant_lines: mutant_lines[mutant_cursor..].to_vec(),
        });
    }

    out
}

fn lcs_line_matches(base_lines: &[String], mutant_lines: &[String]) -> Vec<(usize, usize)> {
    let n = base_lines.len();
    let m = mutant_lines.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];

    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if base_lines[i] == mutant_lines[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    let mut i = 0usize;
    let mut j = 0usize;
    let mut matches = Vec::new();
    while i < n && j < m {
        if base_lines[i] == mutant_lines[j] {
            matches.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    matches
}

fn infer_import_indentation(lines: &[String]) -> String {
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        return leading_whitespace(line);
    }
    String::new()
}

fn parse_rust_variations_ast(input: &str) -> Vec<Span> {
    let Ok(file) = syn::parse_file(input) else {
        return Vec::new();
    };

    let mut visitor = RustMutationVisitor::new(input);
    visitor.visit_file(&file);
    visitor.into_spans()
}

fn parse_rust_variations_legacy(input: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut cursor = 0;

    while let Some(rel_idx) = input[cursor..].find("match") {
        let match_idx = cursor + rel_idx;
        cursor = match_idx + "match".len();

        if !is_word_boundary(input, match_idx, "match".len()) {
            continue;
        }

        let Some(open_brace_idx) = input[cursor..].find('{').map(|idx| cursor + idx) else {
            break;
        };

        let header = &input[cursor..open_brace_idx];
        let Some((variation_name, var_idx_in_header)) = parse_env_var_name(header) else {
            cursor = open_brace_idx + 1;
            continue;
        };

        let Some(close_brace_idx) = find_matching_brace(input, open_brace_idx) else {
            break;
        };

        let body = &input[open_brace_idx + 1..close_brace_idx];
        let variants = extract_ok_variants(body)
            .into_iter()
            .map(|name| Variant {
                name,
                body: VariantBody::InactiveMultiLine {
                    lines: vec![],
                    indentation: String::new(),
                },
            })
            .collect::<Vec<_>>();

        if !variants.is_empty() {
            let base = Variant {
                name: "base".to_string(),
                body: VariantBody::InactiveMultiLine {
                    lines: vec![],
                    indentation: String::new(),
                },
            };

            let variation = Variation {
                name: Some(variation_name),
                tags: vec![],
                base,
                variants,
                active: 0,
                indentation: String::new(),
            };

            let line = line_from_index(input, cursor + var_idx_in_header);
            spans.push(Span::variation(variation, line));
        }

        cursor = close_brace_idx + 1;
    }

    for (line, variation_name, variant_name) in extract_guard_variants(input) {
        if let Some(span) = spans.iter_mut().find(|span| match &span.content {
            crate::code::SpanContent::Variation(v) => v.name.as_deref() == Some(&variation_name),
            _ => false,
        }) {
            if let crate::code::SpanContent::Variation(v) = &mut span.content {
                if !v
                    .variants
                    .iter()
                    .any(|variant| variant.name == variant_name)
                {
                    v.variants.push(Variant {
                        name: variant_name,
                        body: VariantBody::InactiveMultiLine {
                            lines: vec![],
                            indentation: String::new(),
                        },
                    });
                }
            }
        } else {
            let base = Variant {
                name: "base".to_string(),
                body: VariantBody::InactiveMultiLine {
                    lines: vec![],
                    indentation: String::new(),
                },
            };

            let variation = Variation {
                name: Some(variation_name),
                tags: vec![],
                base,
                variants: vec![Variant {
                    name: variant_name,
                    body: VariantBody::InactiveMultiLine {
                        lines: vec![],
                        indentation: String::new(),
                    },
                }],
                active: 0,
                indentation: String::new(),
            };

            spans.push(Span::variation(variation, line));
        }
    }

    spans.sort_by_key(|span| span.line);
    spans
}

struct RustMutationVisitor<'a> {
    _source: &'a str,
    data: Vec<VariationData>,
}

#[derive(Clone, Debug)]
struct VariationData {
    name: Option<String>,
    line: usize,
    variants: Vec<String>,
}

impl<'a> RustMutationVisitor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            _source: source,
            data: Vec::new(),
        }
    }

    fn upsert_variants(
        &mut self,
        name: Option<String>,
        line: usize,
        variants: Vec<String>,
        merge_by_name: bool,
    ) {
        if variants.is_empty() {
            return;
        }

        let existing = if merge_by_name && name.is_some() {
            self.data.iter_mut().find(|v| v.name == name)
        } else {
            self.data
                .iter_mut()
                .find(|v| v.line == line && v.name == name)
        };

        if let Some(existing) = existing {
            for variant in variants {
                if !existing.variants.contains(&variant) {
                    existing.variants.push(variant);
                }
            }
            return;
        }

        self.data.push(VariationData {
            name,
            line,
            variants,
        });
    }

    fn into_spans(mut self) -> Vec<Span> {
        self.data.sort_by_key(|v| v.line);
        self.data
            .into_iter()
            .map(|entry| {
                let base = Variant {
                    name: "base".to_string(),
                    body: VariantBody::InactiveMultiLine {
                        lines: vec![],
                        indentation: String::new(),
                    },
                };
                let variants = entry
                    .variants
                    .into_iter()
                    .map(|name| Variant {
                        name,
                        body: VariantBody::InactiveMultiLine {
                            lines: vec![],
                            indentation: String::new(),
                        },
                    })
                    .collect::<Vec<_>>();
                let variation = Variation {
                    name: entry.name,
                    tags: vec![],
                    base,
                    variants,
                    active: 0,
                    indentation: String::new(),
                };
                Span::variation(variation, entry.line)
            })
            .collect()
    }
}

impl<'ast> Visit<'ast> for RustMutationVisitor<'_> {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if let Some(variation_name) = extract_variation_from_expr(&node.expr) {
            let mut variants = Vec::new();
            for arm in &node.arms {
                collect_variants_from_pat(&arm.pat, &mut variants);
            }
            remove_base_variant(&mut variants);
            dedup_variants(&mut variants);
            self.upsert_variants(
                Some(variation_name),
                node.span().start().line,
                variants,
                true,
            );
        }

        if let Some((variation_name, mut variants, explicit)) =
            extract_variants_from_match_guards(node)
        {
            remove_base_variant(&mut variants);
            dedup_variants(&mut variants);
            self.upsert_variants(variation_name, node.span().start().line, variants, explicit);
        }

        for (variation_name, line, mut variants, explicit) in
            extract_variants_from_match_arm_guards(node)
        {
            remove_base_variant(&mut variants);
            dedup_variants(&mut variants);
            self.upsert_variants(variation_name, line, variants, explicit);
        }

        visit::visit_expr_match(self, node);
    }
}

pub(crate) fn render_rust_functional_code(input: &str, spans: &[Span]) -> anyhow::Result<String> {
    let file = syn::parse_file(input)
        .map_err(|err| anyhow::anyhow!("failed to parse Rust comment source: {err}"))?;
    let index = SourceIndex::new(input);
    let candidates = collect_rust_node_candidates(&file, &index);
    let locations = collect_variation_locations(input, spans, &index)?;

    let mut anonymous_count = 0usize;
    let mut replacements = Vec::new();

    for location in locations {
        anonymous_count += 1;
        let direct = render_rust_functional_variation(&location.variation, anonymous_count);
        if replacement_keeps_file_parseable(input, location.block_range.clone(), &direct) {
            replacements.push(TextReplacement {
                range: location.block_range,
                replacement: direct,
            });
            continue;
        }

        let Some(lifted) = lift_variation_to_node(input, &location, &candidates) else {
            return Err(anyhow::anyhow!(
                "could not find a valid enclosing Rust node for variation at line {}",
                location.line
            ));
        };

        let rendered = render_rust_functional_variation(&lifted.variation, anonymous_count);
        if !replacement_keeps_file_parseable(input, lifted.range.clone(), &rendered) {
            return Err(anyhow::anyhow!(
                "lifted variation at line {} still does not produce valid Rust syntax",
                location.line
            ));
        }

        replacements.push(TextReplacement {
            range: lifted.range,
            replacement: rendered,
        });
    }

    ensure_non_overlapping_replacements(&replacements)?;
    apply_replacements(input, replacements)
        .ok_or_else(|| anyhow::anyhow!("failed to apply Rust functional replacements"))
}

pub(crate) fn render_rust_comment_code_from_functional(input: &str) -> anyhow::Result<String> {
    render_rust_comment_code_from_functional_ast(input)
}

fn render_rust_comment_code_from_functional_ast(input: &str) -> anyhow::Result<String> {
    let file = syn::parse_file(input)
        .map_err(|err| anyhow::anyhow!("failed to parse Rust functional source: {err}"))?;

    let index = SourceIndex::new(input);
    let mut visitor = RustFunctionalToCommentVisitor::new(input, &index);
    visitor.visit_file(&file);
    Ok(apply_replacements(input, visitor.replacements).unwrap_or_else(|| input.to_string()))
}

fn render_rust_comment_code_from_functional_legacy(input: &str) -> String {
    let lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();
    let trailing_newline = input.ends_with('\n');
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < lines.len() {
        if let Some((next_cursor, variation)) = parse_expr_match_block(&lines, cursor) {
            out.extend(render_comment_variation_block(&variation));
            cursor = next_cursor;
            continue;
        }

        if let Some((next_cursor, variation)) = parse_guard_arm_group(&lines, cursor) {
            out.extend(render_comment_variation_block(&variation));
            cursor = next_cursor;
            continue;
        }

        out.push(lines[cursor].clone());
        cursor += 1;
    }

    let mut rendered = out.join("\n");
    if trailing_newline {
        rendered.push('\n');
    }
    rendered
}

#[derive(Debug)]
struct SourceIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceIndex<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (idx, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            source,
            line_starts,
        }
    }

    fn line_start_offset(&self, line: usize) -> Option<usize> {
        if line == 0 {
            return None;
        }
        self.line_starts.get(line - 1).copied()
    }

    fn line_end_offset(&self, line: usize) -> Option<usize> {
        if line == 0 || line > self.line_starts.len() {
            return None;
        }

        let line_start = self.line_starts[line - 1];
        let mut line_end = if line < self.line_starts.len() {
            self.line_starts[line] - 1
        } else {
            self.source.len()
        };

        if line_end > line_start && self.source.as_bytes().get(line_end - 1) == Some(&b'\r') {
            line_end -= 1;
        }

        Some(line_end)
    }

    fn offset_for(&self, loc: proc_macro2::LineColumn) -> Option<usize> {
        if loc.line == 0 || loc.line > self.line_starts.len() {
            return None;
        }

        let line_start = self.line_starts[loc.line - 1];
        let line_end = if loc.line < self.line_starts.len() {
            self.line_starts[loc.line] - 1
        } else {
            self.source.len()
        };
        let line = &self.source[line_start..line_end];
        let line_offset = byte_offset_for_column(line, loc.column)?;
        Some(line_start + line_offset)
    }

    fn range_for_span(&self, span: proc_macro2::Span) -> Option<Range<usize>> {
        let start = self.offset_for(span.start())?;
        let end = self.offset_for(span.end())?;
        if start > end || end > self.source.len() {
            return None;
        }
        Some(start..end)
    }

    fn range_for_span_with_line_indent(&self, span: proc_macro2::Span) -> Option<Range<usize>> {
        let mut range = self.range_for_span(span)?;
        let line_idx = span.start().line.checked_sub(1)?;
        let line_start = *self.line_starts.get(line_idx)?;
        let prefix = self.source.get(line_start..range.start)?;
        if prefix.chars().all(|ch| ch.is_whitespace()) {
            range.start = line_start;
        }
        Some(range)
    }

    fn slice_for_span(&self, span: proc_macro2::Span) -> Option<&'a str> {
        let range = self.range_for_span(span)?;
        self.source.get(range)
    }

    fn indentation_for_span(&self, span: proc_macro2::Span) -> Option<String> {
        let start = self.offset_for(span.start())?;
        let line_idx = span.start().line.checked_sub(1)?;
        let line_start = *self.line_starts.get(line_idx)?;
        let line_prefix = self.source.get(line_start..start)?;
        Some(
            line_prefix
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect(),
        )
    }

    fn has_code_before_span_start(&self, span: proc_macro2::Span) -> Option<bool> {
        let start = self.offset_for(span.start())?;
        let line_idx = span.start().line.checked_sub(1)?;
        let line_start = *self.line_starts.get(line_idx)?;
        let line_prefix = self.source.get(line_start..start)?;
        Some(!line_prefix.trim().is_empty())
    }

    fn line_for_offset(&self, offset: usize) -> Option<usize> {
        if offset > self.source.len() {
            return None;
        }
        let idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        Some(idx + 1)
    }

    fn line_text(&self, line: usize) -> Option<&'a str> {
        let start = self.line_start_offset(line)?;
        let end = self.line_end_offset(line)?;
        self.source.get(start..end)
    }
}

#[derive(Debug)]
struct TextReplacement {
    range: Range<usize>,
    replacement: String,
}

#[derive(Debug, Clone, Default)]
struct FunctionalMetadata {
    variation_name: Option<String>,
    tags: Vec<String>,
}

fn apply_replacements(input: &str, replacements: Vec<TextReplacement>) -> Option<String> {
    if replacements.is_empty() {
        return None;
    }

    let mut rendered = input.to_string();
    let mut replacements = replacements;
    replacements.sort_by(|left, right| right.range.start.cmp(&left.range.start));

    for replacement in replacements {
        if replacement.range.end > rendered.len() || replacement.range.start > replacement.range.end
        {
            continue;
        }
        rendered.replace_range(replacement.range, &replacement.replacement);
    }

    Some(rendered)
}

fn render_functional_metadata_comment(
    variation_name: Option<&str>,
    tags: &[String],
) -> Option<String> {
    if variation_name.is_none() && tags.is_empty() {
        return None;
    }
    let name = variation_name.unwrap_or("");
    let tags = tags.join(",");
    Some(format!("/* marauders:variation={name};tags={tags} */"))
}

fn parse_functional_metadata_comment(line: &str) -> Option<FunctionalMetadata> {
    let trimmed = line.trim();
    if !trimmed.starts_with("/*") || !trimmed.ends_with("*/") {
        return None;
    }

    let marker = "marauders:variation=";
    let marker_idx = trimmed.find(marker)?;
    let rest = &trimmed[(marker_idx + marker.len())..];
    let rest = rest.trim_end();
    let rest = rest.strip_suffix("*/").unwrap_or(rest).trim_end();
    let (variation_part, tail) = match rest.find(';') {
        Some(idx) => (&rest[..idx], &rest[(idx + 1)..]),
        None => (rest, ""),
    };

    let mut metadata = FunctionalMetadata::default();
    let variation = variation_part.trim();
    if !variation.is_empty() {
        metadata.variation_name = Some(variation.to_string());
    }

    if let Some(tags_idx) = tail.find("tags=") {
        let tags_part = &tail[(tags_idx + "tags=".len())..];
        let tags_part = tags_part.split(';').next().unwrap_or(tags_part);
        metadata.tags = tags_part
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(ToString::to_string)
            .collect();
    }

    Some(metadata)
}

fn metadata_before_offset(
    index: &SourceIndex,
    offset: usize,
    expected_indentation: &str,
) -> Option<(FunctionalMetadata, usize)> {
    let line = index.line_for_offset(offset)?;
    if line <= 1 {
        return None;
    }
    let metadata_line = line - 1;
    let text = index.line_text(metadata_line)?;
    if text.trim().is_empty() {
        return None;
    }
    if leading_whitespace(text) != expected_indentation {
        return None;
    }
    let metadata = parse_functional_metadata_comment(text)?;
    let start = index.line_start_offset(metadata_line)?;
    Some((metadata, start))
}

#[derive(Clone, Debug)]
struct VariationLocation {
    line: usize,
    variation: Variation,
    block_range: Range<usize>,
    base_range: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
enum RustNodeKind {
    Expr,
    Arm,
}

#[derive(Clone, Debug)]
struct RustNodeCandidate {
    kind: RustNodeKind,
    range: Range<usize>,
    indentation: String,
}

#[derive(Clone, Debug)]
struct LiftedVariation {
    range: Range<usize>,
    variation: Variation,
}

struct RustNodeCollector<'a> {
    index: &'a SourceIndex<'a>,
    nodes: Vec<RustNodeCandidate>,
}

impl<'a> RustNodeCollector<'a> {
    fn new(index: &'a SourceIndex<'a>) -> Self {
        Self {
            index,
            nodes: Vec::new(),
        }
    }

    fn push_candidate(&mut self, kind: RustNodeKind, span: proc_macro2::Span) {
        let Some(range) = self.index.range_for_span_with_line_indent(span) else {
            return;
        };
        let Some(indentation) = self.index.indentation_for_span(span) else {
            return;
        };
        self.nodes.push(RustNodeCandidate {
            kind,
            range,
            indentation,
        });
    }
}

impl<'ast> Visit<'ast> for RustNodeCollector<'_> {
    fn visit_expr(&mut self, node: &'ast syn::Expr) {
        self.push_candidate(RustNodeKind::Expr, node.span());
        visit::visit_expr(self, node);
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        self.push_candidate(RustNodeKind::Arm, node.span());
        visit::visit_arm(self, node);
    }
}

fn collect_rust_node_candidates(file: &syn::File, index: &SourceIndex) -> Vec<RustNodeCandidate> {
    let mut collector = RustNodeCollector::new(index);
    collector.visit_file(file);
    collector.nodes.sort_by(|left, right| {
        let left_len = left.range.end - left.range.start;
        let right_len = right.range.end - right.range.start;
        left_len
            .cmp(&right_len)
            .then_with(|| left.range.start.cmp(&right.range.start))
    });
    collector.nodes
}

fn collect_variation_locations(
    input: &str,
    spans: &[Span],
    index: &SourceIndex,
) -> anyhow::Result<Vec<VariationLocation>> {
    let mut locations = Vec::new();
    for (idx, span) in spans.iter().enumerate() {
        let crate::code::SpanContent::Variation(variation) = &span.content else {
            continue;
        };

        let start = index.line_start_offset(span.line).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid variation start line {} for Rust conversion",
                span.line
            )
        })?;

        let end = if let Some(next_span) = spans.get(idx + 1) {
            index.line_start_offset(next_span.line).ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid variation end line {} for Rust conversion",
                    next_span.line
                )
            })?
        } else {
            input.len()
        };

        if start >= end || end > input.len() {
            return Err(anyhow::anyhow!(
                "invalid variation byte range for Rust conversion at line {}",
                span.line
            ));
        }

        let base_lines = variation.base.lines();
        if base_lines.is_empty() {
            return Err(anyhow::anyhow!(
                "variation at line {} has an empty base body",
                span.line
            ));
        }
        let base_start_line = span.line + 1;
        let base_end_line = base_start_line + base_lines.len() - 1;
        let base_start = index.line_start_offset(base_start_line).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid base start line {} for Rust conversion",
                base_start_line
            )
        })?;
        let base_end = index.line_end_offset(base_end_line).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid base end line {} for Rust conversion",
                base_end_line
            )
        })?;
        if base_start > base_end || base_end > input.len() {
            return Err(anyhow::anyhow!(
                "invalid base byte range for Rust conversion at line {}",
                span.line
            ));
        }

        locations.push(VariationLocation {
            line: span.line,
            variation: variation.clone(),
            block_range: start..end,
            base_range: base_start..base_end,
        });
    }

    Ok(locations)
}

fn replacement_keeps_file_parseable(input: &str, range: Range<usize>, replacement: &str) -> bool {
    let rendered = apply_replacements(
        input,
        vec![TextReplacement {
            range,
            replacement: replacement.to_string(),
        }],
    );
    rendered
        .as_ref()
        .and_then(|code| syn::parse_file(code).ok())
        .is_some()
}

fn ensure_non_overlapping_replacements(replacements: &[TextReplacement]) -> anyhow::Result<()> {
    let mut sorted = replacements
        .iter()
        .map(|replacement| replacement.range.clone())
        .collect::<Vec<_>>();
    sorted.sort_by_key(|range| range.start);

    for pair in sorted.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(anyhow::anyhow!(
                "overlapping Rust mutation replacements detected during conversion"
            ));
        }
    }

    Ok(())
}

fn lines_from_text(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    text.lines().map(ToString::to_string).collect()
}

fn node_text_is_valid(kind: RustNodeKind, text: &str) -> bool {
    match kind {
        RustNodeKind::Expr => syn::parse_str::<syn::Expr>(text).is_ok(),
        RustNodeKind::Arm => {
            let mut arm_text = text.trim().to_string();
            if !arm_text.ends_with(',') {
                arm_text.push(',');
            }
            let wrapped = format!("match () {{ {arm_text} _ => {{}} }}");
            syn::parse_str::<syn::Expr>(&wrapped).is_ok()
        }
    }
}

fn lift_variation_to_node(
    input: &str,
    location: &VariationLocation,
    candidates: &[RustNodeCandidate],
) -> Option<LiftedVariation> {
    let base_fragment = location.variation.base.lines().join("\n");
    let variant_fragments = location
        .variation
        .variants
        .iter()
        .map(|variant| variant.lines().join("\n"))
        .collect::<Vec<_>>();

    for candidate in candidates {
        if location.base_range.start < candidate.range.start
            || location.base_range.end > candidate.range.end
        {
            continue;
        }

        let hole_start = std::cmp::max(candidate.range.start, location.block_range.start);
        let hole_end = std::cmp::min(candidate.range.end, location.block_range.end);
        if hole_start >= hole_end {
            continue;
        }

        let relative_start = hole_start - candidate.range.start;
        let relative_end = hole_end - candidate.range.start;
        let node_source = input.get(candidate.range.clone())?;
        if relative_start > relative_end || relative_end > node_source.len() {
            continue;
        }

        let prefix = &node_source[..relative_start];
        let suffix = &node_source[relative_end..];
        let base_text = format!("{prefix}{base_fragment}{suffix}");
        if !node_text_is_valid(candidate.kind, &base_text) {
            continue;
        }

        let mut variant_texts = Vec::new();
        let mut all_valid = true;
        for fragment in &variant_fragments {
            let candidate_text = format!("{prefix}{fragment}{suffix}");
            if !node_text_is_valid(candidate.kind, &candidate_text) {
                all_valid = false;
                break;
            }
            variant_texts.push(candidate_text);
        }
        if !all_valid {
            continue;
        }

        let base = Variant {
            name: location.variation.base.name.clone(),
            body: VariantBody::InactiveMultiLine {
                lines: lines_from_text(&base_text),
                indentation: candidate.indentation.clone(),
            },
        };
        let variants = location
            .variation
            .variants
            .iter()
            .zip(variant_texts.iter())
            .map(|(variant, text)| Variant {
                name: variant.name.clone(),
                body: VariantBody::InactiveMultiLine {
                    lines: lines_from_text(text),
                    indentation: candidate.indentation.clone(),
                },
            })
            .collect::<Vec<_>>();
        let variation = Variation {
            name: location.variation.name.clone(),
            tags: location.variation.tags.clone(),
            base,
            variants,
            active: 0,
            indentation: candidate.indentation.clone(),
        };

        let replacement_range = std::cmp::min(location.block_range.start, candidate.range.start)
            ..std::cmp::max(location.block_range.end, candidate.range.end);

        return Some(LiftedVariation {
            range: replacement_range,
            variation,
        });
    }

    None
}

struct RustFunctionalToCommentVisitor<'a> {
    source: &'a str,
    index: &'a SourceIndex<'a>,
    replacements: Vec<TextReplacement>,
}

impl<'a> RustFunctionalToCommentVisitor<'a> {
    fn new(source: &'a str, index: &'a SourceIndex<'a>) -> Self {
        Self {
            source,
            index,
            replacements: Vec::new(),
        }
    }

    fn maybe_replace_expr_match(&mut self, node: &syn::ExprMatch) -> bool {
        let Some(mut range) = self.index.range_for_span_with_line_indent(node.span()) else {
            return false;
        };
        let Some(indentation) = self.index.indentation_for_span(node.span()) else {
            return false;
        };
        let mut metadata = FunctionalMetadata::default();
        if let Some((parsed, start)) = metadata_before_offset(self.index, range.start, &indentation)
        {
            metadata = parsed;
            range.start = start;
        }
        let inline_context = self
            .index
            .has_code_before_span_start(node.span())
            .unwrap_or(false);
        let variation_indentation = if inline_context {
            format!("{indentation}    ")
        } else {
            indentation.clone()
        };

        let source_name = extract_variation_from_expr(&node.expr);
        let mut base_lines = None;
        let mut variants = Vec::new();
        let mut explicit_names = Vec::new();

        if source_name.is_some() {
            for arm in &node.arms {
                match classify_expr_match_arm(&arm.pat) {
                    ExprArmKind::Ignore => {}
                    ExprArmKind::Base => {
                        let Some(lines) = extract_expr_body_lines_for_comment(
                            &arm.body,
                            self.source,
                            self.index,
                            &variation_indentation,
                        ) else {
                            return false;
                        };
                        base_lines = Some(lines);
                    }
                    ExprArmKind::Variant(variant_name) => {
                        let Some(lines) = extract_expr_body_lines_for_comment(
                            &arm.body,
                            self.source,
                            self.index,
                            &variation_indentation,
                        ) else {
                            return false;
                        };
                        variants.push((variant_name, lines));
                    }
                }
            }
        } else {
            for arm in &node.arms {
                if !matches!(arm.pat, syn::Pat::Wild(_)) {
                    return false;
                }

                let Some(lines) = extract_expr_body_lines_for_comment(
                    &arm.body,
                    self.source,
                    self.index,
                    &variation_indentation,
                ) else {
                    return false;
                };

                if let Some((_, guard_expr)) = &arm.guard {
                    let Some((_, mutation)) = strip_mutation_from_guard_expr(guard_expr) else {
                        return false;
                    };
                    if let Some(name) = mutation.variation_name {
                        explicit_names.push(name);
                    }
                    if mutation.variant_name == "base" {
                        base_lines = Some(lines);
                    } else {
                        variants.push((mutation.variant_name, lines));
                    }
                } else {
                    base_lines = Some(lines);
                }
            }
        }

        let Some(base_lines) = base_lines else {
            return false;
        };
        if variants.is_empty() {
            return false;
        }

        let variant_names = variants
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let inferred_name = if let Some(name) = source_name {
            Some(name)
        } else if explicit_names.is_empty() {
            infer_variation_name_from_variants(&variant_names)
        } else if explicit_names.iter().all(|name| name == &explicit_names[0]) {
            Some(explicit_names[0].clone())
        } else {
            None
        };
        let name = metadata.variation_name.clone().or(inferred_name);

        let block = ParsedVariationBlock {
            indentation: variation_indentation,
            name,
            tags: metadata.tags,
            base_lines,
            variants,
        };
        let replacement = if inline_context {
            let mut lines = Vec::new();
            lines.push("{".to_string());
            lines.extend(render_comment_variation_block(&block));
            lines.push(format!("{indentation}}}"));
            lines.join("\n")
        } else {
            render_comment_variation_block(&block).join("\n")
        };
        self.replacements
            .push(TextReplacement { range, replacement });
        true
    }

    fn collect_guard_arm_groups(&self, node: &syn::ExprMatch) -> Vec<GuardArmGroup> {
        let mut groups = Vec::new();
        let mut cursor = 0usize;

        while cursor < node.arms.len() {
            let group_start = cursor;
            let Some(first_arm) =
                parse_guard_arm_from_ast(&node.arms[cursor], self.source, self.index)
            else {
                cursor += 1;
                continue;
            };

            let variation_name = first_arm.variation_name.clone();
            let first_pattern = first_arm.pattern.clone();
            let mut variation_hint = guard_arm_variation_hint(&first_arm);
            let first_range_start = first_arm.range.start;
            let guard_indentation = first_arm
                .lines
                .first()
                .map(|line| leading_whitespace(line))
                .unwrap_or_default();
            let mut arm_data = vec![first_arm];
            cursor += 1;

            while cursor < node.arms.len() {
                let Some(next_arm) =
                    parse_guard_arm_from_ast(&node.arms[cursor], self.source, self.index)
                else {
                    break;
                };
                if let (Some(left), Some(right)) = (&next_arm.variation_name, &variation_name) {
                    if left != right {
                        break;
                    }
                }
                let next_hint = guard_arm_variation_hint(&next_arm);
                if let (Some(left), Some(right)) = (&variation_hint, &next_hint) {
                    if left != right {
                        break;
                    }
                } else if variation_hint.is_none()
                    && next_hint.is_none()
                    && next_arm.pattern != first_pattern
                {
                    break;
                }
                if variation_hint.is_none() {
                    variation_hint = next_hint;
                }
                arm_data.push(next_arm);
                cursor += 1;
            }

            if !arm_data
                .iter()
                .any(|arm| matches!(arm.kind, GuardArmKind::Base))
            {
                if cursor < node.arms.len() {
                    if let Some(base_arm) = parse_base_guard_arm_from_ast(
                        &node.arms[cursor],
                        self.source,
                        self.index,
                        variation_name.clone(),
                    ) {
                        arm_data.push(base_arm);
                        cursor += 1;
                    }
                }
            }

            let metadata =
                metadata_before_offset(self.index, first_range_start, &guard_indentation);
            let metadata_start = metadata.as_ref().map(|(_, start)| *start);
            let metadata = metadata.map(|(metadata, _)| metadata).unwrap_or_default();

            let Some(group) = build_guard_group_replacement(
                variation_name,
                arm_data,
                group_start,
                cursor,
                metadata,
                metadata_start,
            ) else {
                continue;
            };
            groups.push(group);
        }

        groups
    }
}

impl<'ast> Visit<'ast> for RustFunctionalToCommentVisitor<'_> {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if self.maybe_replace_expr_match(node) {
            return;
        }

        let groups = self.collect_guard_arm_groups(node);
        let mut covered = vec![false; node.arms.len()];
        for group in groups {
            for idx in group.arm_start..group.arm_end {
                if let Some(entry) = covered.get_mut(idx) {
                    *entry = true;
                }
            }
            self.replacements.push(group.replacement);
        }

        self.visit_expr(&node.expr);
        for (idx, arm) in node.arms.iter().enumerate() {
            if covered.get(idx).copied().unwrap_or(false) {
                continue;
            }
            self.visit_arm(arm);
        }
    }
}

enum ExprArmKind {
    Base,
    Variant(String),
    Ignore,
}

fn classify_expr_match_arm(pat: &syn::Pat) -> ExprArmKind {
    if matches!(pat, syn::Pat::Wild(_)) {
        return ExprArmKind::Ignore;
    }

    let mut variants = Vec::new();
    collect_variants_from_pat(pat, &mut variants);
    dedup_variants(&mut variants);

    if variants.is_empty() {
        return ExprArmKind::Ignore;
    }

    if variants.iter().any(|variant| variant == "base") {
        ExprArmKind::Base
    } else if variants.len() == 1 {
        ExprArmKind::Variant(variants.remove(0))
    } else {
        ExprArmKind::Ignore
    }
}

fn extract_expr_body_lines_for_comment(
    expr: &syn::Expr,
    source: &str,
    index: &SourceIndex,
    indentation: &str,
) -> Option<Vec<String>> {
    let raw = extract_expr_body_content_lines(expr, source, index)?;
    Some(reindent_lines(&raw, indentation))
}

fn extract_expr_body_content_lines(
    expr: &syn::Expr,
    source: &str,
    index: &SourceIndex,
) -> Option<Vec<String>> {
    let text = source_slice_for_expr(expr, source, index)?;
    if matches!(expr, syn::Expr::Block(_)) {
        let inner = strip_outer_braces(text.trim())?;
        return Some(normalize_body_lines(inner));
    }

    Some(vec![text.trim().to_string()])
}

fn source_slice_for_expr<'a>(
    expr: &syn::Expr,
    source: &'a str,
    index: &SourceIndex<'a>,
) -> Option<&'a str> {
    let range = index.range_for_span(expr.span())?;
    source.get(range)
}

fn strip_outer_braces(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let inner = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    Some(inner)
}

fn normalize_body_lines(input: &str) -> Vec<String> {
    let mut lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.chars().skip(min_indent).collect::<String>()
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GuardMutation {
    variation_name: Option<String>,
    variant_name: String,
}

#[derive(Debug)]
struct GuardArmFromAst {
    variation_name: Option<String>,
    kind: GuardArmKind,
    pattern: String,
    lines: Vec<String>,
    range: Range<usize>,
}

#[derive(Debug)]
struct GuardArmGroup {
    arm_start: usize,
    arm_end: usize,
    replacement: TextReplacement,
}

fn parse_guard_arm_from_ast(
    arm: &syn::Arm,
    source: &str,
    index: &SourceIndex,
) -> Option<GuardArmFromAst> {
    let (_if_token, guard_expr) = arm.guard.as_ref()?;
    let (remaining_guard, mutation) = strip_mutation_from_guard_expr(guard_expr)?;
    let pattern = index.slice_for_span(arm.pat.span())?.trim().to_string();
    let indentation = index.indentation_for_span(arm.span())?;
    let body_lines = extract_expr_body_content_lines(&arm.body, source, index)?;
    let lines = render_guard_arm_lines_for_comment(
        &pattern,
        remaining_guard.as_ref(),
        &indentation,
        &body_lines,
    );
    let range = index.range_for_span_with_line_indent(arm.span())?;

    let kind = if mutation.variant_name == "base" {
        GuardArmKind::Base
    } else {
        GuardArmKind::Variant(mutation.variant_name)
    };

    Some(GuardArmFromAst {
        variation_name: mutation.variation_name,
        kind,
        pattern,
        lines,
        range,
    })
}

fn parse_base_guard_arm_from_ast(
    arm: &syn::Arm,
    source: &str,
    index: &SourceIndex,
    variation_name: Option<String>,
) -> Option<GuardArmFromAst> {
    if arm.guard.is_some() {
        return None;
    }

    let pattern = index.slice_for_span(arm.pat.span())?.trim().to_string();

    let indentation = index.indentation_for_span(arm.span())?;
    let body_lines = extract_expr_body_content_lines(&arm.body, source, index)?;
    let lines = render_guard_arm_lines_for_comment(&pattern, None, &indentation, &body_lines);
    let range = index.range_for_span_with_line_indent(arm.span())?;

    Some(GuardArmFromAst {
        variation_name,
        kind: GuardArmKind::Base,
        pattern,
        lines,
        range,
    })
}

fn guard_arm_variation_hint(arm: &GuardArmFromAst) -> Option<String> {
    if let Some(name) = &arm.variation_name {
        return Some(name.clone());
    }

    match &arm.kind {
        GuardArmKind::Variant(name) => infer_variation_name_from_variants(&[name.clone()]),
        GuardArmKind::Base => None,
    }
}

fn build_guard_group_replacement(
    variation_name: Option<String>,
    arms: Vec<GuardArmFromAst>,
    arm_start: usize,
    arm_end: usize,
    metadata: FunctionalMetadata,
    metadata_start: Option<usize>,
) -> Option<GuardArmGroup> {
    let first = arms.first()?;
    let last = arms.last()?;

    let indentation = leading_whitespace(first.lines.first()?);
    let mut base_lines = None;
    let mut variants = Vec::new();

    for arm in &arms {
        match &arm.kind {
            GuardArmKind::Base => base_lines = Some(arm.lines.clone()),
            GuardArmKind::Variant(name) => variants.push((name.clone(), arm.lines.clone())),
        }
    }

    let variant_names = variants
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let inferred_name =
        variation_name.or_else(|| infer_variation_name_from_variants(&variant_names));
    let name = metadata.variation_name.or(inferred_name);
    let tags = metadata.tags;

    let block = ParsedVariationBlock {
        indentation,
        name,
        tags,
        base_lines: base_lines?,
        variants,
    };

    Some(GuardArmGroup {
        arm_start,
        arm_end,
        replacement: TextReplacement {
            range: metadata_start.unwrap_or(first.range.start)..last.range.end,
            replacement: render_comment_variation_block(&block).join("\n"),
        },
    })
}

fn render_guard_arm_lines_for_comment(
    pattern: &str,
    guard_expr: Option<&syn::Expr>,
    indentation: &str,
    body_lines: &[String],
) -> Vec<String> {
    let guard = guard_expr
        .map(|expr| format!(" if {}", expr.to_token_stream()))
        .unwrap_or_default();
    let mut out = vec![format!("{indentation}{pattern}{guard} => {{")];

    let body_indent = format!("{indentation}    ");
    out.extend(reindent_lines(body_lines, &body_indent));
    out.push(format!("{indentation}}},"));
    out
}

fn strip_mutation_from_guard_expr(expr: &syn::Expr) -> Option<(Option<syn::Expr>, GuardMutation)> {
    let (remaining, mutation) = strip_mutation_from_guard_expr_inner(expr);
    mutation.map(|mutation| (remaining, mutation))
}

fn strip_mutation_from_guard_expr_inner(
    expr: &syn::Expr,
) -> (Option<syn::Expr>, Option<GuardMutation>) {
    if let Some(mutation) = extract_mutation_from_guard_expr(expr) {
        return (None, Some(mutation));
    }

    match expr {
        syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
            let (left_remaining, left_mutation) =
                strip_mutation_from_guard_expr_inner(&binary.left);
            let (right_remaining, right_mutation) =
                strip_mutation_from_guard_expr_inner(&binary.right);

            if left_mutation.is_some()
                && right_mutation.is_some()
                && left_mutation != right_mutation
            {
                return (Some(expr.clone()), None);
            }

            let mutation = left_mutation.or(right_mutation);
            let remaining = match (left_remaining, right_remaining) {
                (Some(left), Some(right)) => Some(parse_quote!(#left && #right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };
            (remaining, mutation)
        }
        syn::Expr::Paren(paren) => strip_mutation_from_guard_expr_inner(&paren.expr),
        syn::Expr::Group(group) => strip_mutation_from_guard_expr_inner(&group.expr),
        _ => (Some(expr.clone()), None),
    }
}

fn extract_mutation_from_guard_expr(expr: &syn::Expr) -> Option<GuardMutation> {
    let macro_expr = match expr {
        syn::Expr::Macro(expr_macro) => expr_macro,
        syn::Expr::Paren(paren) => return extract_mutation_from_guard_expr(&paren.expr),
        syn::Expr::Group(group) => return extract_mutation_from_guard_expr(&group.expr),
        _ => return None,
    };

    if !macro_expr.mac.path.is_ident("matches") {
        return None;
    }

    let Ok(args) = syn::parse2::<MatchesArgs>(macro_expr.mac.tokens.clone()) else {
        return None;
    };
    let mut variants = Vec::new();
    collect_variants_from_pat(&args.pat, &mut variants);
    dedup_variants(&mut variants);

    if variants.iter().any(|variant| variant == "active") {
        let env_name = extract_variation_from_expr(&args.expr)?;
        let (variation_name, variant_name) = split_mutation_env_name(&env_name);
        return Some(GuardMutation {
            variation_name,
            variant_name,
        });
    }

    if variants.is_empty() {
        return None;
    }

    let variation_name = extract_variation_from_expr(&args.expr)?;
    let variant_name = if variants.iter().any(|variant| variant == "base") {
        "base".to_string()
    } else if variants.len() == 1 {
        variants.remove(0)
    } else {
        return None;
    };

    Some(GuardMutation {
        variation_name: Some(variation_name),
        variant_name,
    })
}

fn byte_offset_for_column(line: &str, column: usize) -> Option<usize> {
    if column == 0 {
        return Some(0);
    }

    let mut chars = 0usize;
    for (idx, ch) in line.char_indices() {
        if chars == column {
            return Some(idx);
        }
        chars += 1;
        if ch.len_utf8() > 1 && chars > column {
            return None;
        }
    }

    if chars == column {
        Some(line.len())
    } else {
        None
    }
}

#[derive(Debug)]
struct ParsedVariationBlock {
    indentation: String,
    name: Option<String>,
    tags: Vec<String>,
    base_lines: Vec<String>,
    variants: Vec<(String, Vec<String>)>,
}

fn parse_expr_match_block(lines: &[String], start: usize) -> Option<(usize, ParsedVariationBlock)> {
    let line = lines.get(start)?;
    let trimmed = line.trim();
    if !trimmed.starts_with("match ") {
        return None;
    }

    if trimmed.starts_with("match ()") && trimmed.ends_with('{') {
        return parse_match_unit_block(lines, start);
    }

    let variation = extract_prefixed_env_name(trimmed)?;
    if !trimmed.contains(".as_deref()") || !trimmed.ends_with('{') {
        return None;
    }

    let indentation = leading_whitespace(line);
    let mut cursor = start + 1;
    let mut base_lines = None;
    let mut variants = Vec::new();

    while cursor < lines.len() {
        let current = &lines[cursor];
        let current_trim = current.trim();

        if current_trim == "}" || current_trim == "}," {
            let block = ParsedVariationBlock {
                indentation,
                name: Some(variation),
                tags: vec![],
                base_lines: base_lines?,
                variants,
            };
            return Some((cursor + 1, block));
        }

        if current_trim.starts_with("_ =>") {
            cursor += 1;
            continue;
        }

        let arm_name = parse_ok_variant_name(current_trim)?;
        let (next_cursor, body_lines) = parse_expr_match_arm(lines, cursor)?;
        if arm_name == "base" {
            base_lines = Some(body_lines);
        } else {
            variants.push((arm_name, body_lines));
        }
        cursor = next_cursor;
    }

    None
}

fn parse_match_unit_block(lines: &[String], start: usize) -> Option<(usize, ParsedVariationBlock)> {
    let line = lines.get(start)?;
    let trimmed = line.trim();
    if !trimmed.starts_with("match ()") || !trimmed.ends_with('{') {
        return None;
    }

    let indentation = leading_whitespace(line);
    let mut cursor = start + 1;
    let mut base_lines = None;
    let mut variants: Vec<(String, Vec<String>)> = Vec::new();
    let mut explicit_names: Vec<String> = Vec::new();

    while cursor < lines.len() {
        let current = &lines[cursor];
        let current_trim = current.trim();

        if current_trim == "}" || current_trim == "}," {
            if variants.is_empty() {
                return None;
            }
            let variant_names = variants
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            let name = if explicit_names.is_empty() {
                infer_variation_name_from_variants(&variant_names)
            } else if explicit_names.iter().all(|name| name == &explicit_names[0]) {
                Some(explicit_names[0].clone())
            } else {
                None
            };

            let block = ParsedVariationBlock {
                indentation,
                name,
                tags: vec![],
                base_lines: base_lines?,
                variants,
            };
            return Some((cursor + 1, block));
        }

        if current_trim.starts_with("_ =>") {
            let (next_cursor, body_lines) = parse_expr_match_arm(lines, cursor)?;
            base_lines = Some(body_lines);
            cursor = next_cursor;
            continue;
        }

        if let Some(env_name) = extract_prefixed_env_name(current_trim) {
            let (next_cursor, body_lines) = parse_expr_match_arm(lines, cursor)?;
            let (variation_name, variant_name) = split_mutation_env_name(&env_name);
            if let Some(name) = variation_name {
                explicit_names.push(name);
            }
            variants.push((variant_name, body_lines));
            cursor = next_cursor;
            continue;
        }

        cursor += 1;
    }

    None
}

fn parse_expr_match_arm(lines: &[String], start: usize) -> Option<(usize, Vec<String>)> {
    let line = lines.get(start)?;
    let arrow_idx = line.find("=>")?;
    let suffix = &line[arrow_idx..];
    if !suffix.contains('{') {
        return None;
    }

    let mut cursor = start;
    let mut depth = brace_delta(suffix);
    let mut body = Vec::new();
    cursor += 1;

    while cursor < lines.len() {
        let current = &lines[cursor];
        let delta = brace_delta(current);
        depth += delta;
        if depth == 0 {
            return Some((cursor + 1, body));
        }

        body.push(strip_leading_columns(current, 8));

        cursor += 1;
    }

    None
}

fn parse_guard_arm_group(lines: &[String], start: usize) -> Option<(usize, ParsedVariationBlock)> {
    let (_first_kind, first_name, indentation, _normalized) =
        parse_guard_arm_header(lines.get(start)?)?;
    let mut cursor = start;
    let variation_name = first_name;
    let mut base = None;
    let mut variants = Vec::new();

    while cursor < lines.len() {
        let (kind, name, arm_indent, normalized_first) =
            match parse_guard_arm_header(&lines[cursor]) {
                Some(data) => data,
                None => break,
            };
        if arm_indent != indentation || name != variation_name {
            break;
        }

        let (next_cursor, arm_lines) = parse_guard_arm(lines, cursor, &normalized_first)?;
        match kind {
            GuardArmKind::Base => base = Some(arm_lines),
            GuardArmKind::Variant(variant) => variants.push((variant, arm_lines)),
        }
        cursor = next_cursor;
    }

    let block = ParsedVariationBlock {
        indentation,
        name: Some(variation_name),
        tags: vec![],
        base_lines: base?,
        variants,
    };
    Some((cursor, block))
}

#[derive(Debug, Clone)]
enum GuardArmKind {
    Base,
    Variant(String),
}

fn parse_guard_arm_header(line: &str) -> Option<(GuardArmKind, String, String, String)> {
    let if_idx = line.find("if matches!(")?;
    let arrow_idx = line.find("=>")?;
    let variation_name = extract_prefixed_env_name(line)?;
    let arm_kind =
        if line.contains(r#"Ok("base") | Err(_)"#) || line.contains(r#"Err(_) | Ok("base")"#) {
            GuardArmKind::Base
        } else {
            GuardArmKind::Variant(parse_ok_variant_name(line)?)
        };
    let indentation = leading_whitespace(line);
    let prefix = line[..if_idx].trim_end();
    let suffix = line[arrow_idx..].trim_start();
    let normalized = format!("{prefix} {suffix}");
    Some((arm_kind, variation_name, indentation, normalized))
}

fn parse_guard_arm(
    lines: &[String],
    start: usize,
    normalized_first: &str,
) -> Option<(usize, Vec<String>)> {
    let mut out = Vec::new();
    out.push(normalized_first.to_string());

    let first = lines.get(start)?;
    let arrow_idx = first.find("=>")?;
    let suffix = &first[arrow_idx..];
    let mut depth = brace_delta(suffix);
    if depth == 0 {
        return Some((start + 1, out));
    }
    let mut cursor = start + 1;

    while cursor < lines.len() {
        let current = &lines[cursor];
        out.push(current.clone());
        depth += brace_delta(current);
        if depth == 0 {
            return Some((cursor + 1, out));
        }
        cursor += 1;
    }

    None
}

fn strip_leading_columns(input: &str, columns: usize) -> String {
    let mut taken = 0usize;
    let mut byte_idx = 0usize;
    let mut stopped = false;
    for (idx, ch) in input.char_indices() {
        if taken < columns && (ch == ' ' || ch == '\t') {
            taken += 1;
            byte_idx = idx + ch.len_utf8();
            continue;
        }
        byte_idx = idx;
        stopped = true;
        break;
    }
    if taken < columns {
        input.to_string()
    } else {
        if !stopped {
            String::new()
        } else {
            input[byte_idx..].to_string()
        }
    }
}

fn render_comment_variation_block(block: &ParsedVariationBlock) -> Vec<String> {
    let mut out = Vec::new();
    let title = render_comment_variation_title(block.name.as_deref(), &block.tags);
    if title.is_empty() {
        out.push(format!("{}/*| */", block.indentation));
    } else {
        out.push(format!("{}/*| {} */", block.indentation, title));
    }
    out.extend(block.base_lines.clone());

    for (name, lines) in &block.variants {
        out.push(format!("{}/*|| {} */", block.indentation, name));
        out.push(format!("{}/*|", block.indentation));
        out.extend(lines.clone());
        out.push(format!("{}*/", block.indentation));
    }

    out.push(format!("{}/* |*/", block.indentation));
    out
}

fn render_comment_variation_title(name: Option<&str>, tags: &[String]) -> String {
    let mut title = String::new();
    if let Some(name) = name {
        if !name.is_empty() {
            title.push_str(name);
            if !tags.is_empty() {
                title.push(' ');
            }
        }
    }
    if !tags.is_empty() {
        title.push('[');
        title.push_str(&tags.join(", "));
        title.push(']');
    }
    title
}

fn parse_ok_variant_name(input: &str) -> Option<String> {
    let ok_idx = input.find(r#"Ok(""#)?;
    let start = ok_idx + 4;
    let rest = &input[start..];
    let end_rel = rest.find('"')?;
    Some(rest[..end_rel].to_string())
}

fn extract_prefixed_env_name(input: &str) -> Option<String> {
    let marker = format!(r#""{RUST_MUTATION_ENV_PREFIX}"#);
    let idx = input.find(&marker)?;
    let start = idx + marker.len();
    let rest = &input[start..];
    let end_rel = rest.find('"')?;
    Some(rest[..end_rel].to_string())
}

fn leading_whitespace(input: &str) -> String {
    input.chars().take_while(|c| c.is_whitespace()).collect()
}

fn brace_delta(input: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for ch in input.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn render_rust_functional_variation(variation: &Variation, anonymous_idx: usize) -> String {
    if is_match_arm_variation(variation) {
        return render_rust_functional_match_arms(variation, anonymous_idx);
    }

    let mut output = String::new();
    let indent = &variation.indentation;
    let _ = anonymous_idx;
    let variation_name = variation.name.as_deref();

    if let Some(metadata) = render_functional_metadata_comment(variation_name, &variation.tags) {
        output.push_str(indent);
        output.push_str(&metadata);
        output.push('\n');
    }

    output.push_str(indent);
    output.push_str("match () {\n");

    for variant in &variation.variants {
        let pattern = format!(
            "_ if {}",
            variant_activation_guard(variation_name, &variant.name)
        );
        render_rust_functional_arm(&mut output, indent, &pattern, &variant.lines());
    }

    render_rust_functional_arm(&mut output, indent, "_", &variation.base.lines());
    output.push_str(indent);
    output.push_str("}\n");

    output
}

fn render_rust_functional_match_arms(variation: &Variation, anonymous_idx: usize) -> String {
    let mut output = String::new();
    let _ = anonymous_idx;
    let variation_name = variation.name.as_deref();

    if let Some(metadata) = render_functional_metadata_comment(variation_name, &variation.tags) {
        output.push_str(&variation.indentation);
        output.push_str(&metadata);
        output.push('\n');
    }

    for variant in &variation.variants {
        let variant_guard = variant_activation_guard(variation_name, &variant.name);
        for line in guard_match_arm_lines(&variant.lines(), &variant_guard) {
            output.push_str(&line);
            output.push('\n');
        }
    }
    for line in ensure_guard_arm_base_lines(&variation.base.lines()) {
        output.push_str(&line);
        output.push('\n');
    }

    output
}

fn render_rust_functional_arm(output: &mut String, indent: &str, pattern: &str, lines: &[String]) {
    output.push_str(indent);
    output.push_str("    ");
    output.push_str(pattern);
    output.push_str(" => {\n");

    let body_indent = format!("{indent}        ");
    let lines = reindent_lines(lines, &body_indent);

    for line in lines {
        output.push_str(&line);
        output.push('\n');
    }

    output.push_str(indent);
    output.push_str("    },\n");
}

fn variant_activation_guard(variation_name: Option<&str>, variant_name: &str) -> String {
    let env_var = mutation_env_var_name(variation_name, variant_name);
    format!(r#"matches!(std::env::var({env_var:?}).as_deref(), Ok("active"))"#)
}

fn mutation_env_var_name(variation_name: Option<&str>, variant_name: &str) -> String {
    if let Some(name) = variation_name {
        let expected_prefix = format!("{name}_");
        if variant_name.starts_with(&expected_prefix) {
            return format!("{RUST_MUTATION_ENV_PREFIX}{variant_name}");
        }
        return format!("{RUST_MUTATION_ENV_PREFIX}{name}__{variant_name}");
    }

    format!("{RUST_MUTATION_ENV_PREFIX}{variant_name}")
}

fn ensure_guard_arm_base_lines(lines: &[String]) -> Vec<String> {
    let mut out = lines.to_vec();
    ensure_arm_trailing_comma(&mut out);
    out
}

fn reindent_lines(lines: &[String], target_indent: &str) -> Vec<String> {
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|ch| ch.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                let content = line.chars().skip(min_indent).collect::<String>();
                format!("{target_indent}{content}")
            }
        })
        .collect()
}

fn is_match_arm_variation(variation: &Variation) -> bool {
    lines_look_like_match_arm(&variation.base.lines())
        && variation
            .variants
            .iter()
            .all(|variant| lines_look_like_match_arm(&variant.lines()))
}

fn lines_look_like_match_arm(lines: &[String]) -> bool {
    lines
        .iter()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.contains("=>"))
}

fn guard_match_arm_lines(lines: &[String], guard: &str) -> Vec<String> {
    let mut lines = lines.to_vec();

    if let Some(idx) = lines.iter().position(|line| line.contains("=>")) {
        if let Some(arrow_idx) = lines[idx].find("=>") {
            let before_arrow = &lines[idx][..arrow_idx];
            let replacement = if before_arrow.contains(" if ") {
                format!("&& ({guard}) =>")
            } else {
                format!("if {guard} =>")
            };
            lines[idx] = lines[idx].replacen("=>", &replacement, 1);
        }
    }

    ensure_arm_trailing_comma(&mut lines);
    lines
}

fn ensure_arm_trailing_comma(lines: &mut [String]) {
    if let Some(last_idx) = lines.iter().rposition(|line| !line.trim().is_empty()) {
        if !lines[last_idx].trim_end().ends_with(',') {
            lines[last_idx].push(',');
        }
    }
}

fn extract_variation_from_expr(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Call(call) => {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                if is_env_var_path(&path.path) {
                    let first = call.args.first()?;
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(name),
                        ..
                    }) = first
                    {
                        return strip_env_prefix(name.value());
                    }
                }
            }
            None
        }
        syn::Expr::MethodCall(method) => extract_variation_from_expr(&method.receiver),
        syn::Expr::Paren(paren) => extract_variation_from_expr(&paren.expr),
        syn::Expr::Group(group) => extract_variation_from_expr(&group.expr),
        syn::Expr::Reference(reference) => extract_variation_from_expr(&reference.expr),
        _ => None,
    }
}

fn is_env_var_path(path: &syn::Path) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    segments == vec!["std".to_string(), "env".to_string(), "var".to_string()]
        || segments == vec!["env".to_string(), "var".to_string()]
}

fn strip_env_prefix(name: String) -> Option<String> {
    name.strip_prefix(RUST_MUTATION_ENV_PREFIX)
        .map(ToString::to_string)
}

fn split_mutation_env_name(env_name: &str) -> (Option<String>, String) {
    if let Some((variation, variant)) = env_name.split_once("__") {
        if !variation.is_empty() && !variant.is_empty() {
            return (Some(variation.to_string()), variant.to_string());
        }
    }
    (None, env_name.to_string())
}

fn infer_variation_name_from_variants(variants: &[String]) -> Option<String> {
    if variants.is_empty() {
        return None;
    }

    let mut inferred = None::<String>;
    for variant in variants {
        let (prefix, suffix) = variant.rsplit_once('_')?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        match &inferred {
            Some(existing) if existing != prefix => return None,
            Some(_) => {}
            None => inferred = Some(prefix.to_string()),
        }
    }

    inferred
}

fn collect_variants_from_pat(pat: &syn::Pat, out: &mut Vec<String>) {
    match pat {
        syn::Pat::Or(or_pat) => {
            for case in &or_pat.cases {
                collect_variants_from_pat(case, out);
            }
        }
        syn::Pat::TupleStruct(tuple_struct) => {
            if tuple_struct.path.is_ident("Ok") {
                if let Some(first) = tuple_struct.elems.first() {
                    if let syn::Pat::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(name),
                        ..
                    }) = first
                    {
                        out.push(name.value());
                    }
                }
            }
        }
        syn::Pat::Paren(inner) => collect_variants_from_pat(&inner.pat, out),
        syn::Pat::Reference(inner) => collect_variants_from_pat(&inner.pat, out),
        _ => {}
    }
}

fn remove_base_variant(variants: &mut Vec<String>) {
    variants.retain(|variant| variant != "base");
}

fn dedup_variants(variants: &mut Vec<String>) {
    let mut seen = Vec::new();
    variants.retain(|variant| {
        if seen.contains(variant) {
            false
        } else {
            seen.push(variant.clone());
            true
        }
    });
}

fn extract_variants_from_match_guards(
    node: &syn::ExprMatch,
) -> Option<(Option<String>, Vec<String>, bool)> {
    if !node
        .arms
        .iter()
        .all(|arm| matches!(arm.pat, syn::Pat::Wild(_)))
    {
        return None;
    }

    let mut mutations = Vec::new();
    for arm in &node.arms {
        let Some((_, guard_expr)) = &arm.guard else {
            continue;
        };
        if let Some((_, mutation)) = strip_mutation_from_guard_expr(guard_expr) {
            mutations.push(mutation);
        }
    }

    if mutations.is_empty() {
        return None;
    }

    let mut variants = mutations
        .iter()
        .filter_map(|mutation| {
            if mutation.variant_name == "base" {
                None
            } else {
                Some(mutation.variant_name.clone())
            }
        })
        .collect::<Vec<_>>();
    dedup_variants(&mut variants);
    if variants.is_empty() {
        return None;
    }

    let explicit_names = mutations
        .iter()
        .filter_map(|mutation| mutation.variation_name.clone())
        .collect::<Vec<_>>();
    let (variation_name, explicit) = if explicit_names.is_empty() {
        (infer_variation_name_from_variants(&variants), false)
    } else if explicit_names.iter().all(|name| name == &explicit_names[0]) {
        (Some(explicit_names[0].clone()), true)
    } else {
        (None, false)
    };

    Some((variation_name, variants, explicit))
}

fn extract_variants_from_match_arm_guards(
    node: &syn::ExprMatch,
) -> Vec<(Option<String>, usize, Vec<String>, bool)> {
    if node
        .arms
        .iter()
        .all(|arm| matches!(arm.pat, syn::Pat::Wild(_)))
    {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor < node.arms.len() {
        let arm = &node.arms[cursor];
        let Some((_, guard_expr)) = &arm.guard else {
            cursor += 1;
            continue;
        };
        let Some((_, first_mutation)) = strip_mutation_from_guard_expr(guard_expr) else {
            cursor += 1;
            continue;
        };

        let pattern = arm.pat.to_token_stream().to_string();
        let line = arm.span().start().line;
        let mut variants = Vec::new();
        let mut explicit_names = Vec::new();
        let mut variation_hint = mutation_variation_hint(&first_mutation);
        if let Some(name) = first_mutation.variation_name {
            explicit_names.push(name);
        }
        variants.push(first_mutation.variant_name);
        cursor += 1;

        while cursor < node.arms.len() {
            let arm = &node.arms[cursor];
            let Some((_, guard_expr)) = &arm.guard else {
                break;
            };
            let Some((_, mutation)) = strip_mutation_from_guard_expr(guard_expr) else {
                break;
            };
            let next_pattern = arm.pat.to_token_stream().to_string();
            let next_hint = mutation_variation_hint(&mutation);
            if let (Some(left), Some(right)) = (&variation_hint, &next_hint) {
                if left != right {
                    break;
                }
            } else if variation_hint.is_none() && next_hint.is_none() && next_pattern != pattern {
                break;
            }
            if variation_hint.is_none() {
                variation_hint = next_hint;
            }
            if let Some(name) = mutation.variation_name {
                explicit_names.push(name);
            }
            variants.push(mutation.variant_name);
            cursor += 1;
        }

        remove_base_variant(&mut variants);
        dedup_variants(&mut variants);
        if variants.is_empty() {
            continue;
        }

        let (variation_name, explicit) = if explicit_names.is_empty() {
            (infer_variation_name_from_variants(&variants), false)
        } else if explicit_names.iter().all(|name| name == &explicit_names[0]) {
            (Some(explicit_names[0].clone()), true)
        } else {
            (None, false)
        };
        out.push((variation_name, line, variants, explicit));
    }

    out
}

fn mutation_variation_hint(mutation: &GuardMutation) -> Option<String> {
    mutation
        .variation_name
        .clone()
        .or_else(|| infer_variation_name_from_variants(&[mutation.variant_name.clone()]))
}

fn extract_variation_from_matches_macro(mac: &syn::Macro) -> Option<(String, Vec<String>)> {
    if !mac.path.is_ident("matches") {
        return None;
    }

    let Ok(args) = syn::parse2::<MatchesArgs>(mac.tokens.clone()) else {
        return None;
    };

    let variation_name = extract_variation_from_expr(&args.expr)?;
    let mut variants = Vec::new();
    collect_variants_from_pat(&args.pat, &mut variants);
    Some((variation_name, variants))
}

struct MatchesArgs {
    expr: syn::Expr,
    _comma: syn::Token![,],
    pat: syn::Pat,
}

impl syn::parse::Parse for MatchesArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            expr: input.parse()?,
            _comma: input.parse()?,
            pat: syn::Pat::parse_multi(input)?,
        })
    }
}

fn parse_env_var_name(input: &str) -> Option<(String, usize)> {
    const NEEDLES: [&str; 2] = ["std::env::var", "env::var"];
    let mut search_from = 0;

    while search_from < input.len() {
        let mut found: Option<(usize, &str)> = None;

        for needle in NEEDLES {
            if let Some(rel_idx) = input[search_from..].find(needle) {
                let idx = search_from + rel_idx;
                match found {
                    Some((best_idx, _)) if idx >= best_idx => {}
                    _ => found = Some((idx, needle)),
                }
            }
        }

        let (idx, needle) = found?;
        search_from = idx + needle.len();

        if !is_word_boundary(input, idx, needle.len()) {
            continue;
        }

        let mut cursor = idx + needle.len();
        skip_whitespace(input, &mut cursor);
        if input.as_bytes().get(cursor) != Some(&b'(') {
            continue;
        }

        cursor += 1;
        skip_whitespace(input, &mut cursor);
        if input.as_bytes().get(cursor) != Some(&b'"') {
            continue;
        }

        if let Some((name, _next)) = parse_quoted_string(input, cursor) {
            if let Some(name) = strip_env_prefix(name) {
                return Some((name, idx));
            }
        }
    }

    None
}

fn find_env_var_calls(input: &str) -> Vec<(usize, String)> {
    const NEEDLES: [&str; 2] = ["std::env::var", "env::var"];
    let mut calls = Vec::new();
    let mut search_from = 0;

    while search_from < input.len() {
        let mut found: Option<(usize, &str)> = None;

        for needle in NEEDLES {
            if let Some(rel_idx) = input[search_from..].find(needle) {
                let idx = search_from + rel_idx;
                match found {
                    Some((best_idx, _)) if idx >= best_idx => {}
                    _ => found = Some((idx, needle)),
                }
            }
        }

        let Some((idx, needle)) = found else {
            break;
        };
        search_from = idx + needle.len();

        if !is_word_boundary(input, idx, needle.len()) {
            continue;
        }

        let mut cursor = idx + needle.len();
        skip_whitespace(input, &mut cursor);
        if input.as_bytes().get(cursor) != Some(&b'(') {
            continue;
        }

        cursor += 1;
        skip_whitespace(input, &mut cursor);
        if input.as_bytes().get(cursor) != Some(&b'"') {
            continue;
        }

        if let Some((name, next)) = parse_quoted_string(input, cursor) {
            if let Some(name) = strip_env_prefix(name) {
                calls.push((idx, name));
                search_from = next;
            }
        }
    }

    calls
}

fn extract_guard_variants(input: &str) -> Vec<(usize, String, String)> {
    let mut result = Vec::new();

    for (idx, variation_name) in find_env_var_calls(input) {
        let end = (idx + 200).min(input.len());
        let snippet = &input[idx..end];
        for variant_name in extract_ok_variants(snippet) {
            let line = line_from_index(input, idx);
            result.push((line, variation_name.clone(), variant_name));
        }
    }

    result
}

fn extract_ok_variants(input: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let mut cursor = 0;

    while cursor < input.len() {
        let Some(rel_idx) = input[cursor..].find("Ok") else {
            break;
        };

        let ok_idx = cursor + rel_idx;
        cursor = ok_idx + "Ok".len();

        if !is_word_boundary(input, ok_idx, "Ok".len()) {
            continue;
        }

        let mut pattern_cursor = cursor;
        skip_whitespace(input, &mut pattern_cursor);
        if input.as_bytes().get(pattern_cursor) != Some(&b'(') {
            continue;
        }
        pattern_cursor += 1;
        skip_whitespace(input, &mut pattern_cursor);
        if input.as_bytes().get(pattern_cursor) != Some(&b'"') {
            continue;
        }

        let Some((name, next)) = parse_quoted_string(input, pattern_cursor) else {
            continue;
        };
        cursor = next;

        if name == "base" {
            continue;
        }

        if !variants.contains(&name) {
            variants.push(name);
        }
    }

    variants
}

fn parse_quoted_string(input: &str, quote_idx: usize) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if bytes.get(quote_idx) != Some(&b'"') {
        return None;
    }

    let mut escaped = false;
    let mut cursor = quote_idx + 1;
    while cursor < bytes.len() {
        let b = bytes[cursor];
        if escaped {
            escaped = false;
            cursor += 1;
            continue;
        }

        match b {
            b'\\' => {
                escaped = true;
                cursor += 1;
            }
            b'"' => {
                let content = input[quote_idx + 1..cursor].to_string();
                return Some((content, cursor + 1));
            }
            _ => {
                cursor += 1;
            }
        }
    }

    None
}

fn find_matching_brace(input: &str, open_brace_idx: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.get(open_brace_idx) != Some(&b'{') {
        return None;
    }

    let mut depth = 0usize;
    let mut cursor = open_brace_idx;
    let mut in_string = false;
    let mut escaped = false;

    while cursor < bytes.len() {
        let b = bytes[cursor];

        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            cursor += 1;
            continue;
        }

        match b {
            b'"' => {
                in_string = true;
                cursor += 1;
            }
            b'{' => {
                depth += 1;
                cursor += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => {
                cursor += 1;
            }
        }
    }

    None
}

fn is_word_boundary(input: &str, idx: usize, len: usize) -> bool {
    let before = if idx == 0 {
        None
    } else {
        input[..idx].chars().next_back()
    };
    let after = input[idx + len..].chars().next();

    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    before.map_or(true, |c| !is_ident(c)) && after.map_or(true, |c| !is_ident(c))
}

fn skip_whitespace(input: &str, idx: &mut usize) {
    let bytes = input.as_bytes();
    while *idx < bytes.len() && bytes[*idx].is_ascii_whitespace() {
        *idx += 1;
    }
}

fn line_from_index(input: &str, idx: usize) -> usize {
    input[..idx].bytes().filter(|b| *b == b'\n').count() + 1
}

fn parse(input: &str) -> Result<AST, Box<pest::error::Error<Rule>>> {
    let mut pairs = LispParser::parse(Rule::program, input)?;
    let expressions = pairs.next().unwrap().into_inner();

    let mut ast = Vec::new();
    for expr in expressions {
        if expr.as_rule() == Rule::EOI {
            break;
        }

        if expr.as_rule() == Rule::COMMENT {
            continue;
        }

        ast.push(parse_expression(expr.into_inner().next().unwrap()));
    }

    Ok(AST::Begin(ast))
}

fn parse_expression(pair: pest::iterators::Pair<Rule>) -> AST {
    match pair.as_rule() {
        Rule::define => {
            let mut pairs = pair.into_inner();
            let symbol = pairs.next().unwrap().as_str().to_string();
            let expr = Box::new(parse_expression(pairs.next().unwrap()));
            AST::Define(symbol, expr)
        }
        Rule::number => {
            let number = pair.as_str().parse().unwrap();
            AST::Number(number)
        }
        Rule::symbol => {
            let symbol = pair.as_str().to_string();
            AST::Symbol(symbol)
        }
        Rule::string => {
            let string = pair.as_str().to_string();
            AST::String(string)
        }
        Rule::boolean => {
            let boolean = pair.as_str();
            let boolean = match boolean {
                "#t" => true,
                "#f" => false,
                _ => unreachable!(),
            };
            AST::Boolean(boolean)
        }
        Rule::begin => {
            let mut ast = Vec::new();
            for expr in pair.into_inner() {
                ast.push(parse_expression(expr));
            }
            AST::Begin(ast)
        }
        Rule::call => {
            let mut pairs = pair.into_inner();
            let symbol = pairs.next().unwrap().as_str().to_string();
            let mut args = Vec::new();
            for expr in pairs {
                args.push(parse_expression(expr));
            }
            AST::Call(symbol, args)
        }
        Rule::apply => {
            let pairs = pair.into_inner();
            let mut ast = Vec::new();
            for expr in pairs {
                ast.push(parse_expression(expr));
            }
            AST::Apply(ast)
        }
        Rule::mutate => {
            // (mutate "x" ("x_1" (define x 10)) ("x_2" (define x 20)))
            let mut pair = pair.into_inner();
            let symbol = pair.peek().map(|p| parse_string(p));
            if symbol.is_some() {
                let _ = pair.next();
            }
            let mut mutations = Vec::new();
            for expr in pair {
                let mut pairs = expr.into_inner();
                let name = parse_string(pairs.next().unwrap());
                let expr = parse_expression(pairs.next().unwrap());
                mutations.push((name, expr));
            }
            AST::Mutate(symbol, mutations)
        }
        Rule::lambda => {
            let mut pairs = pair.into_inner();
            let mut args = Vec::new();
            while Rule::symbol == pairs.peek().unwrap().as_rule() {
                args.push(pairs.next().unwrap().as_str().to_string());
            }

            let body = Box::new(parse_expression(pairs.next().unwrap()));
            AST::Lambda(args, body)
        }
        Rule::ite => {
            let mut pairs = pair.into_inner();
            let condition = Box::new(parse_expression(pairs.next().unwrap()));
            let then = Box::new(parse_expression(pairs.next().unwrap()));
            let else_ = Box::new(parse_expression(pairs.next().unwrap()));
            AST::If(condition, then, else_)
        }

        Rule::expression => parse_expression(pair.into_inner().next().unwrap()),
        _ => unimplemented!("Rule not implemented: {:?}", pair.as_rule()),
    }
}

fn parse_string(pair: pest::iterators::Pair<Rule>) -> String {
    match pair.as_rule() {
        Rule::string => {
            let s = pair.as_str();
            s[1..s.len() - 1].to_owned()
        }
        _ => unreachable!("Expected a string, found {:?}", pair.as_rule()),
    }
}

fn eval(ast: AST, ctx: &mut HashMap<String, Value>) -> anyhow::Result<Value> {
    match ast {
        AST::Number(n) => Ok(Value::Number(n)),
        AST::Symbol(s) => ctx
            .get(&s)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Symbol '{s}' not found")),
        AST::String(s) => Ok(Value::String(s)),
        AST::Boolean(b) => Ok(Value::Boolean(b)),
        AST::Define(id, ast) => {
            let value = eval(*ast, ctx)?;
            ctx.insert(id, value.clone());
            Ok(value)
        }
        AST::Lambda(args, body) => Ok(Value::Function(args, body)),
        AST::If(cond, then, else_) => {
            let cond = eval(*cond, ctx)?;
            match cond {
                Value::Boolean(true) => eval(*then, ctx),
                Value::Boolean(false) => eval(*else_, ctx),
                _ => Err(anyhow::anyhow!("Expected a boolean, found {:?}", cond)),
            }
        }
        AST::Begin(exprs) => {
            let mut result = Value::Boolean(false);
            for expr in exprs {
                result = eval(expr, ctx)?;
            }
            Ok(result)
        }
        AST::Call(f, args) => {
            let f = ctx
                .get(&f)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Symbol '{f}' not found"))?;
            match f {
                Value::Function(params, body) => {
                    let mut new_ctx = ctx.clone();
                    for (param, arg) in params.iter().zip(args.iter()) {
                        new_ctx.insert(param.clone(), eval(arg.clone(), ctx)?);
                    }
                    eval(*body, &mut new_ctx)
                }
                Value::Builtin(b) => {
                    let mut args_ = Vec::new();
                    for arg in args {
                        args_.push(eval(arg, ctx)?);
                    }
                    eval_builtin(b, args_)
                }
                _ => Err(anyhow::anyhow!("Expected a function, found {:?}", f)),
            }
        }
        AST::Apply(exprs) => {
            let first = exprs.first().unwrap();
            let mut args = Vec::new();
            for expr in exprs.iter().skip(1) {
                args.push(eval(expr.clone(), ctx)?);
            }

            match eval(first.clone(), ctx)? {
                Value::Function(params, body) => {
                    let mut new_ctx = ctx.clone();
                    for (param, arg) in params.iter().zip(args.iter()) {
                        new_ctx.insert(param.clone(), arg.clone());
                    }
                    eval(*body, &mut new_ctx)
                }
                Value::Builtin(b) => eval_builtin(b, args),
                _ => Err(anyhow::anyhow!("Expected a function, found {:?}", first)),
            }
        }
        AST::Mutate(name, mutations) => {
            // There are 2 ways to enable a mutation
            // 1. If the variation name is not provided, mutations are only applied if
            //   the mutation id is `Boolean(true)` in the context.
            // 2. If the name is provided, then the mutation can be applied
            //   if the variation name is `Number(n)` where n is the index of the mutation.

            let mut result = Value::Boolean(false);
            let mut applied = false;
            // search for the names of the mutations
            for (name, expr) in mutations.iter() {
                if ctx.get(name) == Some(&Value::Boolean(true)) {
                    // found the mutation, apply it
                    result = eval(expr.clone(), ctx)?;
                    applied = true;
                    break;
                }
            }

            if !applied && name.is_some() {
                let name = name.unwrap();
                if let Some(Value::Number(n)) = ctx.get(&name) {
                    if let Some((_, expr)) = mutations.get(*n as usize) {
                        result = eval(expr.clone(), ctx)?;
                    }
                }
            }

            Ok(result)
        }
    }
}

fn eval_builtin(builtin: Builtin, args: Vec<Value>) -> anyhow::Result<Value> {
    match builtin {
        Builtin::Sum => {
            let mut sum = 0;
            for arg in args {
                if let Value::Number(n) = arg {
                    sum += n;
                } else {
                    return Err(anyhow::anyhow!("Expected a number"));
                }
            }
            Ok(Value::Number(sum))
        }
        Builtin::Mult => {
            let mut mult = 1;
            for arg in args {
                if let Value::Number(n) = arg {
                    mult *= n;
                } else {
                    return Err(anyhow::anyhow!("Expected a number"));
                }
            }
            Ok(Value::Number(mult))
        }
        Builtin::And => {
            let mut result = true;
            for arg in args {
                if let Value::Boolean(b) = arg {
                    result = result && b;
                } else {
                    return Err(anyhow::anyhow!("Expected a boolean"));
                }
            }
            Ok(Value::Boolean(result))
        }
        Builtin::Or => {
            let mut result = false;
            for arg in args {
                if let Value::Boolean(b) = arg {
                    result = result || b;
                } else {
                    return Err(anyhow::anyhow!("Expected a boolean"));
                }
            }
            Ok(Value::Boolean(result))
        }
        Builtin::Not => {
            if let Value::Boolean(b) = args[0] {
                Ok(Value::Boolean(!b))
            } else {
                Err(anyhow::anyhow!("Expected a boolean"))
            }
        }
        Builtin::Eq => {
            if args.is_empty() {
                return Ok(Value::Boolean(true));
            }
            let mut result = true;
            let mut prev = args[0].clone();

            for arg in &args[1..] {
                let curr = arg.clone();
                if prev != curr {
                    result = false;
                    break;
                }
                prev = curr;
            }
            Ok(Value::Boolean(result))
        }
        Builtin::Minus => {
            if args.is_empty() {
                return Ok(Value::Number(0));
            }
            let mut result = if let Value::Number(n) = args[0] {
                n
            } else {
                return Err(anyhow::anyhow!("Expected a number"));
            };

            for arg in &args[1..] {
                if let Value::Number(n) = arg {
                    result -= n;
                } else {
                    return Err(anyhow::anyhow!("Expected a number"));
                }
            }
            Ok(Value::Number(result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::SpanContent;

    fn variation_signatures(spans: &[crate::code::Span]) -> Vec<(String, Vec<String>)> {
        spans
            .iter()
            .filter_map(|span| match &span.content {
                SpanContent::Variation(v) => Some((
                    v.name.clone().unwrap_or_default(),
                    v.variants
                        .iter()
                        .map(|variant| variant.name.clone())
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>()
    }

    #[test]
    fn test_parse_rust_variations() {
        let input = r#"
fn add(a: i32, b: i32) -> i32 {
    match std::env::var("M_add_variation").as_deref() {
        Ok("base") | Err(_) => a + b,
        Ok("add_mutation_1") => a - b,
        Ok("add_mutation_2") => a * b,
        _ => panic!("Unknown variation"),
    }
}
"#;

        let spans = parse_rust_variations(input);
        assert_eq!(spans.len(), 1);

        match &spans[0].content {
            SpanContent::Variation(v) => {
                assert_eq!(v.name.as_deref(), Some("add_variation"));
                assert_eq!(
                    v.variants
                        .iter()
                        .map(|variant| variant.name.as_str())
                        .collect::<Vec<_>>(),
                    vec!["add_mutation_1", "add_mutation_2"]
                );
            }
            _ => panic!("expected variation span"),
        }
    }

    #[test]
    fn test_parse_rust_variations_multiple_matches() {
        let input = r#"
fn left(a: i32, b: i32) -> i32 {
    match env::var("M_left_variation").as_deref() {
        Ok("base") | Err(_) => a + b,
        Ok("left_mutation_1") => a - b,
        _ => unreachable!(),
    }
}

fn right(a: i32, b: i32) -> i32 {
    match std::env::var("M_right_variation").as_deref() {
        Ok("base") => a + b,
        Ok("right_mutation_1") => a * b,
        Ok("right_mutation_2") => a / b,
        _ => unreachable!(),
    }
}
"#;

        let spans = parse_rust_variations(input);
        assert_eq!(spans.len(), 2);

        let names = spans
            .iter()
            .map(|span| match &span.content {
                SpanContent::Variation(v) => v.name.clone().unwrap(),
                _ => unreachable!("expected variation span"),
            })
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["left_variation", "right_variation"]);
    }

    #[test]
    fn test_render_rust_functional_code_from_comment_spans() {
        let source = r#"
fn insert(k: i32, k2: i32) -> i32 {
    /*| insert [arith,core] */
    if k < k2 { 1 } else { 2 }
    /*|| insert_1 */
    /*|
    10
    */
    /*|| insert_2 */
    /*|
    20
    */
    /* |*/
}

fn delete(k: i32, k2: i32) -> i32 {
    /*| delete */
    if k < k2 { 1 } else { 2 }
    /*|| delete_4 */
    /*|
    30
    */
    /* |*/
}

fn union(k: i32, k2: i32) -> i32 {
    /*| union */
    if k < k2 { 1 } else { 2 }
    /*|| union_6 */
    /*|
    40
    */
    /* |*/
}
"#;

        let spans = crate::syntax::comment::parse_code(source).unwrap();
        let converted = render_rust_functional_code(source, &spans).unwrap();

        assert!(converted.contains("match () {"));
        assert!(converted.contains("/* marauders:variation=insert;tags=arith,core */"));
        assert!(
            converted.contains(r#"matches!(std::env::var("M_insert_1").as_deref(), Ok("active"))"#)
        );
        assert!(
            converted.contains(r#"matches!(std::env::var("M_delete_4").as_deref(), Ok("active"))"#)
        );
        assert!(
            converted.contains(r#"matches!(std::env::var("M_union_6").as_deref(), Ok("active"))"#)
        );

        let functional_spans = parse_rust_variations(&converted);
        let names = functional_spans
            .iter()
            .map(|span| match &span.content {
                SpanContent::Variation(v) => v.name.clone().unwrap_or_default(),
                _ => unreachable!("expected variation span"),
            })
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["insert", "delete", "union"]);
    }

    #[test]
    fn test_functional_roundtrip_preserves_tags_via_metadata() {
        let source = r#"
fn calc(a: i32, b: i32) -> i32 {
    /*| add [arith,core] */
    a + b
    /*|| add_1 */
    /*|
    a - b
    */
    /* |*/
}
"#;

        let spans = crate::syntax::comment::parse_code(source).unwrap();
        let functional = render_rust_functional_code(source, &spans).unwrap();
        assert!(functional.contains("/* marauders:variation=add;tags=arith,core */"));

        let roundtrip = render_rust_comment_code_from_functional(&functional).unwrap();
        let roundtrip_spans = crate::syntax::comment::parse_code(&roundtrip).unwrap();
        let variation = roundtrip_spans
            .iter()
            .find_map(|span| match &span.content {
                SpanContent::Variation(v) => Some(v),
                _ => None,
            })
            .unwrap();
        assert_eq!(variation.name.as_deref(), Some("add"));
        assert_eq!(
            variation.tags,
            vec!["arith".to_string(), "core".to_string()]
        );
    }

    #[test]
    fn test_render_rust_functional_match_arm_variation() {
        let source = r#"
fn union_(l: i32, r: i32) -> i32 {
    match (l, r) {
        (0, r) => r,
        (l, 0) => l,
        /*| union */
        (l, r) => { l + r }
        /*|| union_1 */
        /*|
        (l, r) => { l - r }
        */
        /* |*/
    }
}
"#;

        let spans = crate::syntax::comment::parse_code(source).unwrap();
        let converted = render_rust_functional_code(source, &spans).unwrap();

        assert!(!converted.contains(r#"match std::env::var("M_union").as_deref() {"#));
        assert!(
            converted.contains(
                r#"if matches!(std::env::var("M_union_1").as_deref(), Ok("active")) => {"#
            ) || converted.contains(
                r#"if matches!(std::env::var("M_union_1").as_deref(), Ok("active")) => { l - r }"#
            )
        );
        assert!(converted.contains("(l, r) => { l + r }"));
    }

    #[test]
    fn test_render_rust_comment_code_from_functional_expr_match() {
        let functional = r#"
fn insert(k: i32, k2: i32) -> i32 {
    match std::env::var("M_insert").as_deref() {
        Ok("base") | Err(_) => {
            if k < k2 { 1 } else { 2 }
        },
        Ok("insert_1") => {
            10
        },
        _ => panic!("Unknown variation"),
    }
}
"#;

        let converted = render_rust_comment_code_from_functional(functional).unwrap();
        assert!(converted.contains("/*| insert */"));
        assert!(converted.contains("/*|| insert_1 */"));
        let spans = crate::syntax::comment::parse_code(&converted).unwrap();
        let names = spans
            .iter()
            .filter_map(|span| match &span.content {
                SpanContent::Variation(v) => v.name.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["insert"]);
    }

    #[test]
    fn test_render_rust_comment_code_from_functional_guard_arms() {
        let functional = r#"
fn union_(l: i32, r: i32) -> i32 {
    match (l, r) {
        (0, r) => r,
        (l, 0) => l,
        (l, r) if matches!(std::env::var("M_union").as_deref(), Ok("base") | Err(_)) => {
            l + r
        },
        (l, r) if matches!(std::env::var("M_union").as_deref(), Ok("union_6")) => {
            l - r
        },
    }
}
"#;

        let converted = render_rust_comment_code_from_functional(functional).unwrap();
        assert!(converted.contains("/*| union */"));
        assert!(converted.contains("/*|| union_6 */"));
        let spans = crate::syntax::comment::parse_code(&converted).unwrap();
        let names = spans
            .iter()
            .filter_map(|span| match &span.content {
                SpanContent::Variation(v) => v.name.clone(),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["union"]);
    }

    #[test]
    fn test_render_rust_comment_code_from_rustfmt_like_functional() {
        let functional = r#"
fn insert(k: i32, k2: i32) -> i32 {
    match std::env::var("M_insert").as_deref() {
        Ok("base") | Err(_) => {
            if k < k2 {
                1
            } else {
                2
            }
        }
        Ok("insert_1") => 10,
        _ => panic!("Unknown variation"),
    }
}

fn union_(l: i32, r: i32) -> i32 {
    match (l, r) {
        (0, r) => r,
        (l, 0) => l,
        (l, r) if matches!(std::env::var("M_union").as_deref(), Ok("base") | Err(_)) => l + r,
        (l, r) if matches!(std::env::var("M_union").as_deref(), Ok("union_6")) => l - r,
    }
}
"#;

        let converted = render_rust_comment_code_from_functional(functional).unwrap();
        assert!(converted.contains("/*| insert */"));
        assert!(converted.contains("/*|| insert_1 */"));
        assert!(converted.contains("/*| union */"));
        assert!(converted.contains("/*|| union_6 */"));
    }

    #[test]
    fn test_roundtrip_comment_boundaries_in_call_and_match_arm_expr() {
        let source = r#"
fn boundary(flag: bool, x: i32, y: i32, t: Option<i32>) -> i32 {
    let chosen = std::cmp::max(
        1,
        /*| call_expr */
        if flag { x } else { y }
        /*|| call_expr_1 */
        /*|
        x + y
        */
        /* |*/
    );

    match (t, chosen) {
        (Some(v), chosen) if v > chosen => v,
        /*| arm_expr */
        (Some(v), chosen) => chosen - v,
        /*|| arm_expr_1 */
        /*|
        (Some(v), chosen) => chosen + v,
        */
        /* |*/
        (None, _) => chosen,
    }
}
"#;

        let original_spans = crate::syntax::comment::parse_code(source).unwrap();
        let expected = variation_signatures(&original_spans);
        assert_eq!(
            expected,
            vec![
                ("call_expr".to_string(), vec!["call_expr_1".to_string()]),
                ("arm_expr".to_string(), vec!["arm_expr_1".to_string()]),
            ]
        );

        let functional = render_rust_functional_code(source, &original_spans).unwrap();
        assert!(functional
            .contains(r#"matches!(std::env::var("M_call_expr_1").as_deref(), Ok("active"))"#));
        assert!(functional
            .contains(r#"matches!(std::env::var("M_arm_expr_1").as_deref(), Ok("active"))"#));

        let roundtrip_comment = render_rust_comment_code_from_functional(&functional).unwrap();
        let roundtrip_spans = crate::syntax::comment::parse_code(&roundtrip_comment).unwrap();
        assert_eq!(variation_signatures(&roundtrip_spans), expected);

        let roundtrip_functional =
            render_rust_functional_code(&roundtrip_comment, &roundtrip_spans).unwrap();
        assert_eq!(
            variation_signatures(&parse_rust_variations(&functional)),
            variation_signatures(&parse_rust_variations(&roundtrip_functional))
        );
    }

    #[test]
    fn test_render_rust_comment_code_from_functional_whitespace_and_guard_splits() {
        let functional = r#"
fn mixed(l: i32, r: i32, flag: bool) -> i32 {
    let expr = match std::env::var("M_expr")
        .as_deref()
    {
        Ok("base") | Err(_) => {
            if flag { l + r } else { l - r }
        }
        Ok("expr_1") => { l * r }
        _ => unreachable!(),
    };

    match (l, r) {
        (l, r)
            if l < r
                && matches!(
                    std::env::var("M_guard")
                        .as_deref(),
                    Ok("base") | Err(_)
                ) => l + r + expr,
        (l, r)
            if l < r
                && matches!(
                    std::env::var("M_guard")
                        .as_deref(),
                    Ok("guard_1")
                ) => l - r + expr,
        _ => expr,
    }
}
"#;

        let converted = render_rust_comment_code_from_functional(functional).unwrap();
        assert!(converted.contains("/*| expr */"));
        assert!(converted.contains("/*|| expr_1 */"));
        assert!(converted.contains("/*| guard */"));
        assert!(converted.contains("/*|| guard_1 */"));
        assert!(converted.contains("(l, r) if l < r => {"));

        let spans = crate::syntax::comment::parse_code(&converted).unwrap();
        let expected = vec![
            ("expr".to_string(), vec!["expr_1".to_string()]),
            ("guard".to_string(), vec!["guard_1".to_string()]),
        ];
        assert_eq!(variation_signatures(&spans), expected);

        let functional_roundtrip = render_rust_functional_code(&converted, &spans).unwrap();
        assert_eq!(
            variation_signatures(&parse_rust_variations(&functional_roundtrip)),
            expected
        );
    }

    #[test]
    fn test_render_rust_comment_code_from_inline_match_expression_arm() {
        let functional = r#"
fn inline_match(t: Option<i32>) -> i32 {
    match t {
        Some(v) => match std::env::var("M_arm").as_deref() {
            Ok("base") | Err(_) => v + 1,
            Ok("arm_1") => v - 1,
            _ => unreachable!(),
        },
        None => 0,
    }
}
"#;

        let converted = render_rust_comment_code_from_functional(functional).unwrap();
        assert!(converted.contains("Some(v) => {"));
        assert!(converted.contains("/*| arm */"));
        assert!(converted.contains("/*|| arm_1 */"));
        assert!(syn::parse_file(&converted).is_ok());

        let spans = crate::syntax::comment::parse_code(&converted).unwrap();
        let expected = vec![("arm".to_string(), vec!["arm_1".to_string()])];
        assert_eq!(variation_signatures(&spans), expected);

        let functional_roundtrip = render_rust_functional_code(&converted, &spans).unwrap();
        assert_eq!(
            variation_signatures(&parse_rust_variations(&functional_roundtrip)),
            expected
        );
    }

    #[test]
    fn test_roundtrip_fragment_call_arguments_not_ast_unit() {
        let source = r#"
fn frag_call(a: i32, b: i32, c: i32) -> i32 {
    sum(
        /*| call */
        a, b, c
        /*|| wrong_call */
        /*|
        a, b
        */
        /* |*/
    )
}
"#;

        let original_spans = crate::syntax::comment::parse_code(source).unwrap();
        let expected = vec![("call".to_string(), vec!["wrong_call".to_string()])];
        assert_eq!(variation_signatures(&original_spans), expected);

        let functional = render_rust_functional_code(source, &original_spans).unwrap();
        assert_eq!(functional.matches("sum(").count(), 2);
        assert!(syn::parse_file(&functional).is_ok());
        assert!(functional
            .contains(r#"matches!(std::env::var("M_call__wrong_call").as_deref(), Ok("active"))"#));

        let roundtrip_comment = render_rust_comment_code_from_functional(&functional).unwrap();
        let roundtrip_spans = crate::syntax::comment::parse_code(&roundtrip_comment).unwrap();
        assert_eq!(variation_signatures(&roundtrip_spans), expected);
    }

    #[test]
    fn test_lifts_fragment_that_includes_call_closing_paren() {
        let source = r#"
fn frag_call_paren(a: i32, b: i32, c: i32) -> i32 {
    f(
        /*| call */
        a, b, c)
        /*|| wrong_call */
        /*|
        a, b)
        */
        /* |*/
}
"#;

        let spans = crate::syntax::comment::parse_code(source).unwrap();
        let functional = render_rust_functional_code(source, &spans).unwrap();

        assert_eq!(functional.matches("\n            f(").count(), 2);
        assert!(syn::parse_file(&functional).is_ok());
        assert!(functional
            .contains(r#"matches!(std::env::var("M_call__wrong_call").as_deref(), Ok("active"))"#));
    }

    #[test]
    fn test_import_rust_mutants_roundtrip_single_hunk_multi_variants() {
        let base = r#"
fn calc(a: i32, b: i32) -> i32 {
    a + b
}
"#;
        let mutant_1 = r#"
fn calc(a: i32, b: i32) -> i32 {
    a - b
}
"#;
        let mutant_2 = r#"
fn calc(a: i32, b: i32) -> i32 {
    a * b
}
"#;

        let imported = import_rust_mutants_from_files(
            base,
            &[mutant_1.to_string(), mutant_2.to_string()],
            "tool",
        )
        .unwrap();

        assert!(imported.contains("/*| tool_1 */"));
        assert!(imported.contains("/*|| tool_1_1 */"));
        assert!(imported.contains("/*|| tool_1_2 */"));

        let spans = crate::syntax::comment::parse_code(&imported).unwrap();
        assert_eq!(
            variation_signatures(&spans),
            vec![(
                "tool_1".to_string(),
                vec!["tool_1_1".to_string(), "tool_1_2".to_string()]
            )]
        );

        let functional = render_rust_functional_code(&imported, &spans).unwrap();
        assert!(syn::parse_file(&functional).is_ok());
        let roundtrip_comment = render_rust_comment_code_from_functional(&functional).unwrap();
        let roundtrip_spans = crate::syntax::comment::parse_code(&roundtrip_comment).unwrap();
        assert_eq!(
            variation_signatures(&roundtrip_spans),
            variation_signatures(&spans)
        );
    }

    #[test]
    fn test_import_rust_mutants_multiple_hunks() {
        let base = r#"
fn calc(a: i32, b: i32) -> i32 {
    a + b
}

fn cmp(a: i32, b: i32) -> bool {
    a < b
}
"#;
        let mutant = r#"
fn calc(a: i32, b: i32) -> i32 {
    a - b
}

fn cmp(a: i32, b: i32) -> bool {
    a <= b
}
"#;

        let imported = import_rust_mutants_from_files(base, &[mutant.to_string()], "ext").unwrap();
        let spans = crate::syntax::comment::parse_code(&imported).unwrap();
        assert_eq!(
            variation_signatures(&spans),
            vec![
                ("ext_1".to_string(), vec!["ext_1_1".to_string()]),
                ("ext_2".to_string(), vec!["ext_2_1".to_string()]),
            ]
        );
    }

    #[test]
    fn test_parser_simple() {
        let input = "(define x 10)";
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Define(
                "x".to_string(),
                Box::new(AST::Number(10))
            )])
        );
    }

    #[test]
    fn test_parser_nested() {
        let input = "(define x (define y 10))";
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Define(
                "x".to_string(),
                Box::new(AST::Define("y".to_string(), Box::new(AST::Number(10))))
            )])
        );
    }

    #[test]
    fn test_parser_multiple() {
        let input = "(define x 10) (define y 20)";
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![
                AST::Define("x".to_string(), Box::new(AST::Number(10))),
                AST::Define("y".to_string(), Box::new(AST::Number(20)))
            ])
        );
    }

    #[test]
    fn test_parser_call() {
        let input = "(f 10 20)";
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Call(
                "f".to_string(),
                vec![AST::Number(10), AST::Number(20)]
            )])
        );
    }

    #[test]
    fn test_parser_apply() {
        let input = "((+ 10) 20)";
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Apply(vec![
                AST::Call("+".to_string(), vec![AST::Number(10)]),
                AST::Number(20)
            ])])
        );
    }

    #[test]
    fn test_parser_mutate() {
        let input = r#"(mutate "x" ("x_1" (define x 10)) ("x_2" (define x 20)))"#;
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Mutate(
                Some("x".to_string()),
                vec![
                    (
                        "x_1".to_string(),
                        AST::Define("x".to_string(), Box::new(AST::Number(10)))
                    ),
                    (
                        "x_2".to_string(),
                        AST::Define("x".to_string(), Box::new(AST::Number(20)))
                    )
                ]
            )])
        );
    }

    #[test]
    fn test_parser_comment() {
        let input = r#"
#| This is a comment |#
(define x 10)
"#;
        let ast = parse(input).unwrap();
        assert_eq!(
            ast,
            AST::Begin(vec![AST::Define(
                "x".to_string(),
                Box::new(AST::Number(10))
            )])
        );
    }

    #[test]
    fn test_eval_simple() {
        let input = r#"
(define id (lambda (x) x))
(id 3)
"#;
        let ast = parse(input).unwrap();
        let result = eval(ast, &mut HashMap::new()).unwrap();
        assert_eq!(result, Value::Number(3))
    }

    #[test]
    fn test_eval_builtin() {
        let input = r#"
(define sum (lambda (x y) (+ x y)))
(sum 3 4)
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("+".to_string(), Value::Builtin(Builtin::Sum));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Number(7))
    }

    #[test]
    fn test_eval_mutate() {
        let input = r#"
(mutate "x" ("x_1" (define x 20)) ("x_2" (define x 30)))
x
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("x_1".to_string(), Value::Boolean(true));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Number(20))
    }

    #[test]
    fn test_eval_if() {
        let input = r#"
(+ (if #t 10 20) (if #f 30 40))
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("+".to_string(), Value::Builtin(Builtin::Sum));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Number(50))
    }

    #[test]
    fn test_eval_eq() {
        let input = r#"
(eq 10 10 10)
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("eq".to_string(), Value::Builtin(Builtin::Eq));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Boolean(true))
    }

    #[test]
    fn test_eval_anonymous() {
        let input = r#"
((lambda (x y) (+ x y)) 10 20)
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("+".to_string(), Value::Builtin(Builtin::Sum));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Number(30))
    }

    #[test]
    fn test_eval_recursion() {
        let input = r#"
(define fact (lambda (n) (if (eq n 0) 1 (* n (fact (- n 1))))))
(fact 5)
"#;
        let ast = parse(input).unwrap();
        let mut ctx = HashMap::new();
        ctx.insert("eq".to_string(), Value::Builtin(Builtin::Eq));
        ctx.insert("*".to_string(), Value::Builtin(Builtin::Mult));
        ctx.insert("-".to_string(), Value::Builtin(Builtin::Minus));
        let result = eval(ast, &mut ctx).unwrap();
        assert_eq!(result, Value::Number(120))
    }
}
