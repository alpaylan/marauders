use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::code::{Span, SpanContent};
use crate::languages::Language;

const FORMAT_TAG: &str = "marauders_patch_bundle";
const MANIFEST_FILE: &str = "manifest.toml";
const PATCH_EXTENSION: &str = "patch";
const DIFF_FILE: &str = "marauders_base.rs";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatchBundleManifest {
    format: String,
    source: String,
    #[serde(default)]
    variations: Vec<PatchVariationMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PatchVariationMeta {
    key: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
struct PatchBlock {
    patch: String,
    old_start: usize,
    old_count: usize,
}

#[derive(Debug, Clone)]
struct ResolvedVariation {
    name: Option<String>,
    tags: Vec<String>,
    old_start: usize,
    old_count: usize,
    variants: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct PatchBundleFile {
    pub(crate) relative_path: PathBuf,
    pub(crate) content: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PatchBundleRender {
    pub(crate) base_source: String,
    pub(crate) manifest: String,
    pub(crate) files: Vec<PatchBundleFile>,
}

pub(crate) fn looks_like_mutations(input: &str) -> bool {
    parse_manifest(input).is_ok()
}

pub(crate) fn patch_bundle_dir_for_source(source_path: &Path) -> anyhow::Result<PathBuf> {
    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("invalid source filename"))?;
    Ok(source_path.with_file_name(format!("{file_name}.patches")))
}

pub(crate) fn write_patch_bundle(
    bundle_dir: &Path,
    rendered: &PatchBundleRender,
) -> anyhow::Result<PathBuf> {
    if bundle_dir.exists() {
        std::fs::remove_dir_all(bundle_dir)?;
    }
    std::fs::create_dir_all(bundle_dir)?;

    let manifest_path = bundle_dir.join(MANIFEST_FILE);
    std::fs::write(&manifest_path, &rendered.manifest)?;

    for file in &rendered.files {
        let full_path = bundle_dir.join(&file.relative_path);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(full_path, &file.content)?;
    }

    Ok(manifest_path)
}

pub(crate) fn render_patch_bundle_from_comment(
    spans: &[Span],
    source_path: &str,
) -> anyhow::Result<PatchBundleRender> {
    let mut base_source = String::new();
    let mut variations = Vec::new();
    let mut files = Vec::new();
    let mut current_line = 1usize;

    for span in spans {
        match &span.content {
            SpanContent::Line(line) => {
                base_source.push_str(line);
                current_line += count_lines(line);
            }
            SpanContent::Variation(variation) => {
                let start_line = current_line;
                let base_lines = variation.base.lines();
                for line in &base_lines {
                    base_source.push_str(line);
                    base_source.push('\n');
                }
                current_line += base_lines.len();

                let variation_key =
                    encode_variation_key(variations.len(), variation.name.as_deref());
                for (variant_index, variant) in variation.variants.iter().enumerate() {
                    let patch = render_unified_patch(start_line, &base_lines, &variant.lines());
                    let variant_stem = encode_variant_stem(variant_index, &variant.name);
                    let relative_path = PathBuf::from(&variation_key)
                        .join(format!("{variant_stem}.{PATCH_EXTENSION}"));
                    files.push(PatchBundleFile {
                        relative_path,
                        content: patch,
                    });
                }

                variations.push(PatchVariationMeta {
                    key: variation_key,
                    tags: variation.tags.clone(),
                });
            }
        }
    }

    let manifest = PatchBundleManifest {
        format: FORMAT_TAG.to_string(),
        source: source_path.to_string(),
        variations,
    };
    let manifest = toml::to_string_pretty(&manifest).map_err(|e| anyhow!(e))?;

    Ok(PatchBundleRender {
        base_source,
        manifest,
        files,
    })
}

pub(crate) fn render_comment_code_from_patch(
    manifest_path: &Path,
    input: &str,
) -> anyhow::Result<(PathBuf, String)> {
    let manifest = parse_manifest(input)?;
    let source_path = PathBuf::from(&manifest.source);
    let bundle_dir = manifest_path.parent().ok_or_else(|| {
        anyhow!(
            "invalid patch manifest path '{}': no parent directory",
            manifest_path.display()
        )
    })?;

    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("rs");
    let language = Language::extension_to_language(extension, &vec![]).unwrap_or(Language::Rust);

    let source = std::fs::read_to_string(&source_path).map_err(|e| {
        anyhow!(
            "failed to read source file '{}' referenced by patch manifest: {}",
            source_path.display(),
            e
        )
    })?;
    let (mut lines, trailing_newline) = split_lines_preserving_tail(&source);

    let tags_by_key = manifest
        .variations
        .into_iter()
        .map(|variation| (variation.key, variation.tags))
        .collect::<HashMap<_, _>>();
    let mut resolved = load_resolved_variations(bundle_dir, &tags_by_key)?;
    resolved.sort_by(|l, r| {
        r.old_start
            .cmp(&l.old_start)
            .then(r.old_count.cmp(&l.old_count))
    });

    for variation in resolved {
        validate_range(variation.old_start, variation.old_count, lines.len())?;
        let start = variation.old_start - 1;
        let end_exclusive = start + variation.old_count;
        let base_fragment = lines[start..end_exclusive].to_vec();
        let indent = infer_indentation(&base_fragment);

        let block = render_comment_variation_block(
            &language,
            variation.name.as_deref(),
            &variation.tags,
            &indent,
            &base_fragment,
            &variation
                .variants
                .iter()
                .map(|(name, replacement)| (name.as_str(), replacement.as_slice()))
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

fn load_resolved_variations(
    bundle_dir: &Path,
    tags_by_key: &HashMap<String, Vec<String>>,
) -> anyhow::Result<Vec<ResolvedVariation>> {
    let mut variations = Vec::new();

    for entry in std::fs::read_dir(bundle_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let key = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("non-unicode variation key in patch bundle"))?;
        let (variation_order, variation_name) = parse_variation_key(&key)?;

        let mut patch_blocks = Vec::new();
        for patch_entry in std::fs::read_dir(entry.path())? {
            let patch_entry = patch_entry?;
            if !patch_entry.file_type()?.is_file() {
                continue;
            }
            let path = patch_entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(PATCH_EXTENSION) {
                continue;
            }

            let file_stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| anyhow!("invalid patch file name '{}'", path.display()))?;
            let (variant_order, variant_name) = parse_variant_stem(file_stem)?;
            let patch = std::fs::read_to_string(&path)?;
            let (old_start, old_count) = parse_patch_old_range(&patch)?;

            patch_blocks.push((
                variant_order,
                variant_name,
                PatchBlock {
                    patch,
                    old_start,
                    old_count,
                },
            ));
        }

        if patch_blocks.is_empty() {
            continue;
        }

        patch_blocks.sort_by(|l, r| l.0.cmp(&r.0));
        let first_start = patch_blocks[0].2.old_start;
        let first_count = patch_blocks[0].2.old_count;
        let mut variants = Vec::new();
        for (_order, variant_name, block) in patch_blocks {
            if block.old_start != first_start || block.old_count != first_count {
                bail!(
                    "variant '{}' hunk range mismatch in variation '{}': expected -{},{} got -{},{}",
                    variant_name,
                    variation_name.as_deref().unwrap_or("<anonymous>"),
                    first_start,
                    first_count,
                    block.old_start,
                    block.old_count
                );
            }
            let replacement = parse_patch_replacement(&block.patch, first_start, first_count)?;
            variants.push((variant_name, replacement));
        }

        variations.push((
            variation_order,
            ResolvedVariation {
                name: variation_name,
                tags: tags_by_key.get(&key).cloned().unwrap_or_default(),
                old_start: first_start,
                old_count: first_count,
                variants,
            },
        ));
    }

    variations.sort_by(|l, r| l.0.cmp(&r.0));
    let variations = variations
        .into_iter()
        .map(|(_order, variation)| variation)
        .collect::<Vec<_>>();

    if variations.is_empty() {
        bail!("patch bundle '{}' has no patch files", bundle_dir.display());
    }

    Ok(variations)
}

fn parse_manifest(input: &str) -> anyhow::Result<PatchBundleManifest> {
    let manifest: PatchBundleManifest = toml::from_str(input).map_err(|e| anyhow!(e))?;
    if manifest.format != FORMAT_TAG {
        bail!("unsupported patch format '{}'", manifest.format);
    }
    if manifest.source.trim().is_empty() {
        bail!("patch manifest source cannot be empty");
    }
    Ok(manifest)
}

fn encode_variation_key(order: usize, name: Option<&str>) -> String {
    let encoded_name = match name {
        Some(name) => format!("s_{}", encode_component(name)),
        None => "n".to_string(),
    };
    format!("{order:03}__{encoded_name}")
}

fn parse_variation_key(key: &str) -> anyhow::Result<(usize, Option<String>)> {
    let (order, encoded_name) = key
        .split_once("__")
        .ok_or_else(|| anyhow!("invalid variation key '{}'", key))?;
    let order = order.parse::<usize>()?;
    if encoded_name == "n" {
        return Ok((order, None));
    }
    let encoded_name = encoded_name
        .strip_prefix("s_")
        .ok_or_else(|| anyhow!("invalid variation key '{}'", key))?;
    Ok((order, Some(decode_component(encoded_name)?)))
}

fn encode_variant_stem(order: usize, name: &str) -> String {
    format!("{order:03}__{}", encode_component(name))
}

fn parse_variant_stem(stem: &str) -> anyhow::Result<(usize, String)> {
    let (order, encoded_name) = stem.split_once("__").ok_or_else(|| {
        anyhow!(
            "invalid patch variant file '{}': expected '<order>__<name>'",
            stem
        )
    })?;
    let order = order.parse::<usize>()?;
    Ok((order, decode_component(encoded_name)?))
}

fn encode_component(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        if is_safe_component_byte(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

fn decode_component(encoded: &str) -> anyhow::Result<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                bail!("invalid percent-encoding in '{}'", encoded);
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|e| anyhow!("invalid percent-encoding in '{}': {}", encoded, e))?;
            let value = u8::from_str_radix(hex, 16)
                .map_err(|e| anyhow!("invalid percent-encoding in '{}': {}", encoded, e))?;
            out.push(value);
            index += 3;
            continue;
        }

        out.push(bytes[index]);
        index += 1;
    }

    String::from_utf8(out).map_err(|e| anyhow!("invalid utf8 while decoding '{}': {}", encoded, e))
}

fn is_safe_component_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
}

fn render_unified_patch(start_line: usize, before: &[String], after: &[String]) -> String {
    let mut patch = String::new();
    patch.push_str("diff --git a/");
    patch.push_str(DIFF_FILE);
    patch.push_str(" b/");
    patch.push_str(DIFF_FILE);
    patch.push('\n');
    patch.push_str("--- a/");
    patch.push_str(DIFF_FILE);
    patch.push('\n');
    patch.push_str("+++ b/");
    patch.push_str(DIFF_FILE);
    patch.push('\n');
    patch.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        start_line,
        before.len(),
        start_line,
        after.len()
    ));
    for line in before {
        patch.push('-');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in after {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

fn parse_patch_old_range(patch: &str) -> anyhow::Result<(usize, usize)> {
    let mut found = None;
    for line in patch.lines() {
        if !line.starts_with("@@ ") {
            continue;
        }
        if found.is_some() {
            bail!("multiple hunks are not supported in patch conversion");
        }
        found = Some(parse_hunk_old_range(line)?);
    }
    found.ok_or_else(|| anyhow!("patch block does not contain a hunk"))
}

fn parse_patch_replacement(
    patch: &str,
    expected_start: usize,
    expected_old_count: usize,
) -> anyhow::Result<Vec<String>> {
    let mut in_hunk = false;
    let mut replacement = Vec::new();
    let mut found_hunk = false;

    for line in patch.lines() {
        if line.starts_with("@@ ") {
            if found_hunk {
                bail!("multiple hunks are not supported in patch conversion");
            }
            let (old_start, old_count) = parse_hunk_old_range(line)?;
            if old_start != expected_start || old_count != expected_old_count {
                bail!(
                    "patch hunk range mismatch: expected -{},{} got -{},{}",
                    expected_start,
                    expected_old_count,
                    old_start,
                    old_count
                );
            }
            in_hunk = true;
            found_hunk = true;
            continue;
        }

        if !in_hunk {
            continue;
        }

        if line.starts_with('+') && !line.starts_with("+++") {
            replacement.push(line[1..].to_string());
        } else if line.starts_with(' ') {
            replacement.push(line[1..].to_string());
        } else if line.starts_with('-') {
            continue;
        } else if line.starts_with('\\') {
            continue;
        } else if line.starts_with("@@ ") {
            bail!("multiple hunks are not supported in patch conversion");
        }
    }

    if !found_hunk {
        bail!("patch block does not contain a hunk");
    }

    Ok(replacement)
}

fn parse_hunk_old_range(header: &str) -> anyhow::Result<(usize, usize)> {
    let middle = header
        .split("@@")
        .nth(1)
        .map(str::trim)
        .ok_or_else(|| anyhow!("invalid hunk header '{}'", header))?;
    let old = middle
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("invalid hunk header '{}'", header))?;
    parse_hunk_range(old, '-')
}

fn parse_hunk_range(token: &str, prefix: char) -> anyhow::Result<(usize, usize)> {
    let range = token
        .strip_prefix(prefix)
        .ok_or_else(|| anyhow!("invalid hunk range '{}'", token))?;
    if let Some((start, count)) = range.split_once(',') {
        Ok((start.parse()?, count.parse()?))
    } else {
        Ok((range.parse()?, 1))
    }
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

fn validate_range(start_line: usize, old_count: usize, line_count: usize) -> anyhow::Result<()> {
    if start_line == 0 {
        bail!("invalid start_line=0 in patch metadata");
    }
    let start = start_line - 1;
    if start > line_count {
        bail!(
            "invalid start line {} for base with {} lines",
            start_line,
            line_count
        );
    }
    let end_exclusive = start + old_count;
    if end_exclusive > line_count {
        bail!(
            "invalid range -{},{} for base with {} lines",
            start_line,
            old_count,
            line_count
        );
    }
    Ok(())
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

    #[test]
    fn test_comment_patch_comment_roundtrip() {
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
            std::env::temp_dir().join(format!("marauders_patch_source_{pid}_{nanos}.rs"));
        let bundle_dir = patch_bundle_dir_for_source(&source_path).unwrap();

        let rendered =
            render_patch_bundle_from_comment(&spans, &source_path.to_string_lossy()).unwrap();
        assert!(rendered
            .manifest
            .contains("format = \"marauders_patch_bundle\""));
        assert!(rendered.manifest.contains("tags = [\"arith\"]"));
        assert!(!rendered.manifest.contains("base ="));

        std::fs::write(&source_path, &rendered.base_source).unwrap();
        let manifest_path = write_patch_bundle(&bundle_dir, &rendered).unwrap();
        let manifest_text = std::fs::read_to_string(&manifest_path).unwrap();

        let (roundtrip_path, roundtrip) =
            render_comment_code_from_patch(&manifest_path, &manifest_text).unwrap();
        assert_eq!(roundtrip_path, source_path);
        assert!(roundtrip.contains("/*| add [arith] */"));
        assert!(roundtrip.contains("/*|| add_1 */"));
        assert!(roundtrip.contains("/*|| add_2 */"));

        let _ = std::fs::remove_file(source_path);
        let _ = std::fs::remove_dir_all(bundle_dir);
    }
}
