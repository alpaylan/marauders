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
    render_unified_patch_for(DIFF_FILE, start_line, before, after)
}

/// Render a unified-diff hunk addressing a real source file path (relative to
/// the project root). The result is `git apply`-compatible from the project
/// root: it produces the same edit as `marauders set --variant <id>` against
/// the comment-form source.
pub(crate) fn render_unified_patch_for(
    source_rel_path: &str,
    start_line: usize,
    before: &[String],
    after: &[String],
) -> String {
    let mut patch = String::new();
    patch.push_str("diff --git a/");
    patch.push_str(source_rel_path);
    patch.push_str(" b/");
    patch.push_str(source_rel_path);
    patch.push('\n');
    patch.push_str("--- a/");
    patch.push_str(source_rel_path);
    patch.push('\n');
    patch.push_str("+++ b/");
    patch.push_str(source_rel_path);
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

/// Etna-format flat patch list: one `.patch` per variant, named by the
/// variant id, addressing the real source path. No manifest, no bundle dir.
#[derive(Debug, Clone)]
pub(crate) struct EtnaPatchFile {
    pub(crate) variant_id: String,
    pub(crate) content: String,
}

/// Reverse of `render_etna_patches_from_comment`: read flat
/// `<project_root>/patches/*.patch` files that address `source_path`, group
/// them by their hunk anchor, and splice marauders comment blocks back into
/// the source. Returns `Ok(None)` when no etna-format patches address this
/// source (so the caller can fall through to the next conversion strategy).
pub(crate) fn try_render_comment_from_etna_patches(
    source_path: &Path,
    source_content: &str,
) -> anyhow::Result<Option<String>> {
    let cwd = std::env::current_dir()?;
    try_render_comment_from_etna_patches_at(source_path, source_content, &cwd)
}

/// Same as `try_render_comment_from_etna_patches` but with an explicit project
/// root, useful for tests that mustn't touch process-global cwd.
pub(crate) fn try_render_comment_from_etna_patches_at(
    source_path: &Path,
    source_content: &str,
    project_root: &Path,
) -> anyhow::Result<Option<String>> {
    let mut search = project_root;
    let patches_dir = loop {
        let candidate = search.join("patches");
        if candidate.is_dir() {
            break candidate;
        }
        match search.parent() {
            Some(parent) if parent != search => search = parent,
            _ => return Ok(None),
        }
    };

    // Path the patches address — relative to project root. Canonicalize both
    // sides so macOS' /tmp -> /private/tmp symlink doesn't break strip_prefix.
    let root_canon = project_root.canonicalize().unwrap_or(project_root.to_path_buf());
    let path_canon = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let rel_source = path_canon
        .strip_prefix(&root_canon)
        .or_else(|_| source_path.strip_prefix(project_root))
        .unwrap_or(source_path)
        .to_string_lossy()
        .into_owned();

    // Walk patches/, parse each, keep the ones that address `rel_source`.
    struct ParsedEtnaPatch {
        variant_id: String,
        old_start: usize,
        ctx_before: Vec<String>,
        before_change: Vec<String>,
        after_change: Vec<String>,
        variation_name: Option<String>,
        tags: Vec<String>,
    }
    // Collect patch paths first and sort so the variant ordering is
    // deterministic across runs and matches the forward conversion's
    // input order (which was source order, naturally lexicographic for
    // names like add_1, add_2, ...).
    let mut patch_paths: Vec<std::path::PathBuf> = std::fs::read_dir(&patches_dir)?
        .filter_map(|e| {
            let p = e.ok()?.path();
            if p.extension().and_then(|e| e.to_str()) == Some("patch") {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    patch_paths.sort();

    let mut parsed: Vec<ParsedEtnaPatch> = Vec::new();
    for p in patch_paths {
        let body = std::fs::read_to_string(&p)?;
        let Some((
            diff_path,
            old_start,
            _old_count,
            ctx_before,
            before_change,
            after_change,
            _ctx_after,
            variation_name,
            tags,
        )) = parse_single_hunk_patch(&body)?
        else {
            continue;
        };
        if diff_path != rel_source {
            continue;
        }
        let variant_id = p
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("invalid patch filename '{}'", p.display()))?
            .to_string();
        parsed.push(ParsedEtnaPatch {
            variant_id,
            old_start,
            ctx_before,
            before_change,
            after_change,
            variation_name,
            tags,
        });
    }

    if parsed.is_empty() {
        return Ok(None);
    }

    use std::collections::BTreeMap;
    let mut groups: BTreeMap<(usize, Vec<String>), Vec<ParsedEtnaPatch>> = BTreeMap::new();
    for p in parsed {
        let anchor = p.old_start + p.ctx_before.len();
        let key = (anchor, p.before_change.clone());
        groups.entry(key).or_default().push(p);
    }

    let (mut lines, trailing_newline) = split_lines_preserving_tail(source_content);
    let mut sorted_groups: Vec<((usize, Vec<String>), Vec<ParsedEtnaPatch>)> =
        groups.into_iter().collect();
    sorted_groups.sort_by(|l, r| r.0 .0.cmp(&l.0 .0));

    for ((anchor_line_1based, before_change), variants) in sorted_groups {
        let start = anchor_line_1based - 1;
        let end_exclusive = start + before_change.len();
        if end_exclusive > lines.len() {
            bail!(
                "patch anchor {}..{} exceeds source length {}",
                anchor_line_1based,
                end_exclusive,
                lines.len()
            );
        }
        let base_fragment: Vec<String> = lines[start..end_exclusive].to_vec();
        let indent = infer_indentation(&base_fragment);
        let language = Language::Rust;

        // All variants of one group share the same variation_name + tags
        // (they came from a single comment block).
        let variation_name = variants.iter().find_map(|v| v.variation_name.clone());
        let tags: Vec<String> = variants
            .iter()
            .find(|v| !v.tags.is_empty())
            .map(|v| v.tags.clone())
            .unwrap_or_default();

        let variant_pairs: Vec<(&str, &[String])> = variants
            .iter()
            .map(|v| (v.variant_id.as_str(), v.after_change.as_slice()))
            .collect();
        let block = render_comment_variation_block(
            &language,
            variation_name.as_deref(),
            &tags,
            &indent,
            &base_fragment,
            &variant_pairs,
        );
        lines.splice(start..end_exclusive, block);
    }

    let mut output = lines.join("\n");
    if trailing_newline {
        output.push('\n');
    }
    Ok(Some(output))
}

/// Parse a single-hunk unified diff produced by `render_unified_patch_with_context`.
/// Returns (target_path, hunk_start_line, old_count, ctx_before, removed, added, ctx_after,
///          variation_name, tags) — name and tags are pulled from the optional
/// `# marauders: name=<n> tags=[<csv>]` comment line that may precede the diff.
fn parse_single_hunk_patch(
    body: &str,
) -> anyhow::Result<
    Option<(
        String,
        usize,
        usize,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Option<String>,
        Vec<String>,
    )>,
> {
    let mut target_path: Option<String> = None;
    let mut hunk_seen = false;
    let mut start = 0usize;
    let mut old_count = 0usize;
    let mut variation_name: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();

    let mut leading_ctx: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut trailing_ctx: Vec<String> = Vec::new();
    let mut state = 0usize;

    for line in body.lines() {
        if !hunk_seen {
            if let Some(meta) = line.strip_prefix("# marauders:") {
                // parse `name=<n>` and `tags=[a,b,c]` segments
                let s = meta.trim();
                for part in split_marauders_meta(s) {
                    if let Some(n) = part.strip_prefix("name=") {
                        variation_name = Some(n.to_string());
                    } else if let Some(t) = part.strip_prefix("tags=") {
                        let t = t.trim_start_matches('[').trim_end_matches(']');
                        tags = t
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
            } else if let Some(rest) = line.strip_prefix("+++ b/") {
                target_path = Some(rest.to_string());
            } else if line.starts_with("@@ ") {
                let (s, c) = parse_hunk_old_range(line)?;
                start = s;
                old_count = c;
                hunk_seen = true;
                state = 1;
            }
            continue;
        }

        if line.starts_with("@@ ") {
            return Ok(None);
        }

        if let Some(rest) = line.strip_prefix(' ') {
            match state {
                1 => leading_ctx.push(rest.to_string()),
                2 => {
                    state = 3;
                    trailing_ctx.push(rest.to_string());
                }
                3 => trailing_ctx.push(rest.to_string()),
                _ => {}
            }
        } else if let Some(rest) = line.strip_prefix('-') {
            if rest.starts_with("--") {
                continue;
            }
            removed.push(rest.to_string());
            state = 2;
        } else if let Some(rest) = line.strip_prefix('+') {
            if rest.starts_with("++") {
                continue;
            }
            added.push(rest.to_string());
            state = 2;
        }
    }

    let Some(path) = target_path else {
        return Ok(None);
    };
    if !hunk_seen {
        return Ok(None);
    }
    let _ = old_count;
    Ok(Some((
        path,
        start,
        leading_ctx.len() + removed.len() + trailing_ctx.len(),
        leading_ctx,
        removed,
        added,
        trailing_ctx,
        variation_name,
        tags,
    )))
}

/// Split `name=foo tags=[a,b]` while respecting the `[ ]` group.
fn split_marauders_meta(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_brackets = false;
    for ch in s.chars() {
        match ch {
            '[' => {
                in_brackets = true;
                buf.push(ch);
            }
            ']' => {
                in_brackets = false;
                buf.push(ch);
            }
            ' ' if !in_brackets => {
                if !buf.is_empty() {
                    out.push(std::mem::take(&mut buf));
                }
            }
            _ => buf.push(ch),
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

pub(crate) fn render_etna_patches_from_comment(
    spans: &[Span],
    source_rel_path: &str,
) -> anyhow::Result<(String, Vec<EtnaPatchFile>)> {
    // First pass: build the base source and remember (start_line, base_lines, variants, name, tags)
    // for each variation so the second pass can splice context from base_source.
    let mut base_source = String::new();
    let mut current_line = 1usize;
    struct PendingVariation {
        start_line: usize,
        base_lines: Vec<String>,
        variants: Vec<(String, Vec<String>)>,
        name: Option<String>,
        tags: Vec<String>,
    }
    let mut pending: Vec<PendingVariation> = Vec::new();

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

                let variants = variation
                    .variants
                    .iter()
                    .map(|v| (v.name.clone(), v.lines()))
                    .collect();

                pending.push(PendingVariation {
                    start_line,
                    base_lines,
                    variants,
                    name: variation.name.clone(),
                    tags: variation.tags.clone(),
                });
            }
        }
    }

    // Second pass: gather 3-line context before/after each variation from base_source
    // and emit a `git apply`-compatible unified diff with a marauders metadata header.
    const CONTEXT: usize = 3;
    let base_lines_all: Vec<&str> = base_source.lines().collect();
    let mut patches = Vec::new();

    for pv in &pending {
        let removed_start_idx = pv.start_line.saturating_sub(1);
        let ctx_before_start = removed_start_idx.saturating_sub(CONTEXT);
        let ctx_before: Vec<String> = base_lines_all[ctx_before_start..removed_start_idx]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let after_idx = removed_start_idx + pv.base_lines.len();
        let ctx_after_end = (after_idx + CONTEXT).min(base_lines_all.len());
        let ctx_after: Vec<String> = base_lines_all[after_idx..ctx_after_end]
            .iter()
            .map(|s| s.to_string())
            .collect();

        for (variant_id, variant_lines) in &pv.variants {
            let mut content = String::new();
            // Marauders metadata header — git apply ignores lines before
            // the first `diff --git`, so this is a safe place to encode the
            // variation name + tags for round-tripping back to comment form.
            if pv.name.is_some() || !pv.tags.is_empty() {
                content.push_str("# marauders:");
                if let Some(n) = &pv.name {
                    content.push_str(&format!(" name={}", n));
                }
                if !pv.tags.is_empty() {
                    content.push_str(&format!(" tags=[{}]", pv.tags.join(",")));
                }
                content.push('\n');
            }
            content.push_str(&render_unified_patch_with_context(
                source_rel_path,
                ctx_before_start + 1,
                &ctx_before,
                &pv.base_lines,
                variant_lines,
                &ctx_after,
            ));
            patches.push(EtnaPatchFile {
                variant_id: variant_id.clone(),
                content,
            });
        }
    }

    Ok((base_source, patches))
}

/// Unified diff with N-line context, addressing the real source path.
fn render_unified_patch_with_context(
    source_rel_path: &str,
    hunk_start_line: usize,
    ctx_before: &[String],
    before_change: &[String],
    after_change: &[String],
    ctx_after: &[String],
) -> String {
    let old_count = ctx_before.len() + before_change.len() + ctx_after.len();
    let new_count = ctx_before.len() + after_change.len() + ctx_after.len();

    let mut patch = String::new();
    patch.push_str("diff --git a/");
    patch.push_str(source_rel_path);
    patch.push_str(" b/");
    patch.push_str(source_rel_path);
    patch.push('\n');
    patch.push_str("--- a/");
    patch.push_str(source_rel_path);
    patch.push('\n');
    patch.push_str("+++ b/");
    patch.push_str(source_rel_path);
    patch.push('\n');
    patch.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        hunk_start_line, old_count, hunk_start_line, new_count
    ));
    for line in ctx_before {
        patch.push(' ');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in before_change {
        patch.push('-');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in after_change {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    for line in ctx_after {
        patch.push(' ');
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

        if (line.starts_with('+') && !line.starts_with("+++")) || line.starts_with(' ') {
            replacement.push(line[1..].to_string());
        } else if line.starts_with('-') || line.starts_with('\\') {
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
