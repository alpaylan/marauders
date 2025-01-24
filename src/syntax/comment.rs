use pest::iterators::Pair;
use pest::Parser as _;
use pest_derive::Parser;

use crate::code::{Span, SpanContent};
use crate::variation::{Variant, Variation};

#[derive(Parser)]
#[grammar = "syntax/comment.pest"]
pub(crate) struct Parser;

pub(crate) fn parse_code(input: &str) -> anyhow::Result<Vec<Span>> {
    let mut pairs = Parser::parse(Rule::program, input)?;
    let pairs = pairs
        .next()
        .unwrap()
        .into_inner()
        .next()
        .unwrap()
        .into_inner();
    let mut spans = vec![];

    let mut line = 1;
    for pair in pairs {
        line += parse_span(pair, &mut spans, line);
    }

    Ok(spans)
}

fn parse_span(
    pair: pest::iterators::Pair<Rule>,
    spans: &mut Vec<crate::code::Span>,
    line: usize,
) -> usize {
    match pair.as_rule() {
        Rule::line | Rule::last_line => {
            if spans.is_empty() {
                spans.push(Span::constant(pair.as_str().to_string(), line));
            } else {
                let last = spans.last_mut().unwrap();
                match &last.content {
                    SpanContent::Line(c) => {
                        last.content = SpanContent::Line(format!("{}{}", c, pair.as_str()));
                    }
                    SpanContent::Variation(_) => {
                        spans.push(Span::constant(pair.as_str().to_string(), line));
                    }
                }
            }
            pair.as_str().lines().count()
        }
        Rule::mutation => {
            let mut pair = pair.into_inner();
            let pair = pair.next().unwrap();
            let (variation, current_lines) = parse_variation(pair);
            spans.push(Span::variation(variation, line));
            current_lines
        }
        _ => {
            unreachable!("unexpected rule {:?}", pair.as_rule());
        }
    }
}

fn parse_variation(pair: pest::iterators::Pair<Rule>) -> (Variation, usize) {
    let mut pairs = pair.into_inner();
    let header = pairs.next().unwrap();

    let (name, tags, variation_indentation) = parse_variation_header(header);
    let base = pairs.next().unwrap();

    let (base, active, base_indentation) = parse_base(base);
    
    let mut variants = vec![];

    for pair in pairs {
        if pair.as_rule() == Rule::variation_end {
            break;
        }

        assert!(pair.as_rule() == Rule::variant);

        variants.push(parse_variant(pair));
    }

    // only one of the variants or the base can be active
    let actives = variants.iter().filter(|(_, active)| *active).count();
    let active = if active {
        assert_eq!(actives, 0);
        0
    } else {
        assert_eq!(actives, 1);
        variants.iter().position(|(_, active)| *active).unwrap() + 1
    };

    let mut lines = 0;
    // Begin marker (*! *)
    lines += 1;
    // Base code
    lines += base.len();
    // Variant codes
    for (variant, _) in &variants {
        // Begin marker (*!! *)
        lines += 1;
        // Variant code
        lines += variant.code.len();
    }
    // End marker (* !*)
    lines += 1;
    // Inline markers for the passive variants
    lines += variants.len() * 2;

    let base = (base, base_indentation.unwrap_or(variation_indentation.clone()));

    (
        Variation {
            name,
            tags,
            base,
            variants: variants.into_iter().map(|(v, _)| v).collect(),
            active,
            indentation: variation_indentation,
        },
        lines,
    )
}

fn parse_variation_header(pair: pest::iterators::Pair<Rule>) -> (Option<String>, Vec<String>, String) {
    let mut pairs = pair.into_inner();

    let (indentation, begin_marker) = next2(&mut pairs, Rule::indent).unwrap();
    let indentation = indentation.map(|pair| pair.as_str().to_string()).unwrap_or_default();
    assert_eq!(begin_marker.as_rule(), Rule::variation_begin_marker);

    let maybe_name = pairs.peek().unwrap();
    let name = match maybe_name.as_rule() {
        Rule::identifier => {
            // Move the iterator
            let name = maybe_name.as_str().to_string();
            pairs.next().unwrap();
            Some(name)
        }
        Rule::comment_end | Rule::tags => None,
        p => unreachable!("Unexpected rule {:?}", p),
    };

    let maybe_tags = pairs.peek().unwrap();
    let tags: Vec<String> = match maybe_tags.as_rule() {
        Rule::tags => {
            let tags = maybe_tags
                .into_inner()
                .map(|pair| pair.as_str().to_string())
                .collect();

            // Move the iterator
            pairs.next().unwrap();

            tags
        }
        Rule::comment_end => vec![],
        p => unreachable!("Unexpected rule {:?}", p),
    };

    let end_marker = pairs.next().unwrap();

    assert_eq!(end_marker.as_rule(), Rule::comment_end);

    (name, tags, indentation)
}

fn parse_base(pair: pest::iterators::Pair<Rule>) -> (Vec<String>, bool, Option<String>) {
    let mut pairs = pair.into_inner();
    let body = pairs.next().unwrap();
    parse_variant_body(body)
}

fn get_indent(ws: &str) -> String {
    ws.chars().filter(|c| *c == ' ' || *c == '\t').collect()
}

fn parse_variant(pair: pest::iterators::Pair<Rule>) -> (crate::variation::Variant, bool) {
    let mut pairs = pair.into_inner();
    let header = pairs.next().unwrap();
    let (name, indent) = parse_variant_header(header);

    let body = pairs.next().unwrap();

    let (code, is_active, _indent) = parse_variant_body(body);

    (Variant { name, code, indentation: indent }, is_active)
}

fn parse_variant_header(pair: pest::iterators::Pair<Rule>) -> (String, String) {
    let mut pairs = pair.into_inner();

    let (indentation, begin_marker) = next2(&mut pairs, Rule::indent).unwrap();

    let indentation = indentation.map(|pair| pair.as_str().to_string()).unwrap_or_default();

    assert_eq!(begin_marker.as_rule(), Rule::variant_begin_marker);

    let name = pairs.next().unwrap();
    let name = name.as_str().to_string();

    let end_marker = pairs.next().unwrap();

    assert_eq!(end_marker.as_rule(), Rule::comment_end);

    assert_eq!(pairs.next(), None);
    (name, indentation)
}

fn next2<'a>(
    pairs: &'a mut pest::iterators::Pairs<Rule>,
    rule: Rule,
) -> Option<(Option<Pair<'a, Rule>>, Pair<'a, Rule>)> {
    let maybe_r1 = pairs.peek();
    if let Some(r1) = maybe_r1 {
        pairs.next();
        if r1.as_rule() == rule {
            let r2 = pairs.next().unwrap();
            Some((Some(r1), r2))
        } else {
            Some((None, r1))
        }
    } else {
        None
    }
}

fn parse_variant_body(pair: pest::iterators::Pair<Rule>) -> (Vec<String>, bool, Option<String>) {
    let mut body = pair.into_inner();
    let body = body.next().unwrap();

    match body.as_rule() {
        Rule::inactive_variant_body => {
            let mut pairs = body.into_inner();

            let (indentation, begin_marker) = next2(&mut pairs, Rule::indent).unwrap();
            let indentation = indentation.map(|pair| pair.as_str().to_string()).unwrap_or_default();
            assert_eq!(begin_marker.as_rule(), Rule::variant_body_begin_marker);

            let body = pairs.next().unwrap();
            assert_eq!(body.as_rule(), Rule::comment_text);

            let body = body
                .into_inner()
                .map(|pair| pair.as_str().strip_suffix("\n").unwrap().to_string())
                .collect();

            let (_, end_marker) = next2(&mut pairs, Rule::indent).unwrap();
            assert_eq!(end_marker.as_rule(), Rule::comment_end);

            (body, false, Some(indentation))
        }
        Rule::active_variant_body => {
            let body = body
                .into_inner()
                .into_iter()
                .map(|pair| pair.as_str().strip_suffix("\n").unwrap().to_string())
                .collect();

            (body, true, None)
        }
        p => unreachable!("unexpected rule {:?}", p),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, result};

    use crate::code::{Code, SpanContent};

    use super::*;

    #[test]
    fn test_parse_variation_begin_marker() {
        let result = Parser::parse(Rule::variation_begin_marker, r#"(*!"#)
            .unwrap()
            .next()
            .unwrap();

        assert!(result.as_str() == "(*!");
    }

    #[test]
    fn test_parse_variation_header() {
        let result = Parser::parse(Rule::variation_header, r#"(*! delete_4 *)"#)
            .unwrap()
            .next()
            .unwrap();
        let result = parse_variation_header(result);

        assert_eq!(result.0, Some("delete_4".to_string()));
    }

    #[test]
    fn test_parse_variation_header_noid() {
        let result = Parser::parse(Rule::variation_header, r#"(*! *)"#)
            .unwrap()
            .next()
            .unwrap();
        let result = parse_variation_header(result);

        assert_eq!(result.0, None);
        assert_eq!(result.1, Vec::new() as Vec<String>);
    }

    #[test]
    fn test_variation_end() {
        let result = Parser::parse(Rule::variation_end, r#"(* !*)"#)
            .unwrap()
            .next()
            .unwrap();

        assert_eq!(result.as_str(), "(* !*)");
    }

    #[test]
    fn test_variation_end_whitespace() {
        let result = Parser::parse(Rule::variation_end, r#"(*! *)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_variation_end_whitespace2() {
        let result = Parser::parse(Rule::variation_end, r#"(* ! *)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_variation_base() {
        let result = Parser::parse(
            Rule::base,
            r#"
if k <? k' then T (delete k l) k' v' r
else if k' <? k then T l k' v' (delete k r)
else join l r
"#,
        )
        .unwrap()
        .next()
        .unwrap();

        let result = parse_base(result);

        assert_eq!(
            result.0,
            vec![
                "",
                "if k <? k' then T (delete k l) k' v' r",
                "else if k' <? k then T l k' v' (delete k r)",
                "else join l r",
            ]
        );

        assert_eq!(result.1, true);
    }

    #[test]
    fn test_variant_header() {
        let result = Parser::parse(Rule::variant_header, r#"  (*!! delete_4 *)"#)
            .unwrap()
            .next()
            .unwrap();

        let (name, indent) = parse_variant_header(result);
        assert_eq!(name, "delete_4");
        assert_eq!(indent, "  ");
    }

    #[test]
    fn test_variant_body_begin() {
        let result = Parser::parse(Rule::variant_body_begin_marker, r#"(*!"#)
            .unwrap()
            .next()
            .unwrap();

        assert_eq!(result.as_str(), "(*!");
    }

    #[test]
    fn test_variant_body_end() {
        let result = Parser::parse(Rule::comment_end, r#"*)"#)
            .unwrap()
            .next()
            .unwrap();

        assert_eq!(result.as_str(), "*)");
    }

    #[test]
    fn test_variant_body() {
        let mut result = Parser::parse(
            Rule::variant_body,
            r#"(*!
if k <? k' then delete k l
else if k' <? k then delete k r
else join l r
*)
"#,
        )
        .unwrap();

        let result = result.next().unwrap();

        let result = parse_variant_body(result);
        assert_eq!(
            result.0,
            vec![
                "if k <? k' then delete k l",
                "else if k' <? k then delete k r",
                "else join l r"
            ]
        );

        assert_eq!(result.1, false);
    }

    #[test]
    fn test_variant2() {
        let mut result = Parser::parse(
            Rule::variant,
            r#"(*!! delete_4 *)
(*!
if k <? k' then delete k l
else if k' <? k then delete k r
else join l r
*)
"#,
        )
        .unwrap();

        let result = result.next().unwrap();

        let result = parse_variant(result);

        assert_eq!(result.0.name, "delete_4");
        assert_eq!(
            result.0.code,
            vec![
                "if k <? k' then delete k l",
                "else if k' <? k then delete k r",
                "else join l r",
            ]
        );
        assert_eq!(result.1, false);
    }

    #[test]
    fn test_variation2() {
        let result = Parser::parse(
            Rule::variation,
            r#"(*! *)
  if k <? k' then T (delete k l) k' v' r
  else if k' <? k then T l k' v' (delete k r)
  else join l r
  (*!! delete_4 *)
  (*!
  if k <? k' then delete k l
  else if k' <? k then delete k r
  else join l r
  *)
  (*!! delete_5 *)
  (*!
  if k' <? k then T (delete k l) k' v' r
  else if k <? k' then T l k' v' (delete k r)
  else join l r
  *)
  (* !*)"#,
        )
        .unwrap()
        .next()
        .unwrap();

        let (variation, line) = parse_variation(result);

        assert_eq!(
            variation.base.0,
            vec![
                "  if k <? k' then T (delete k l) k' v' r",
                "  else if k' <? k then T l k' v' (delete k r)",
                "  else join l r",
            ]
        );

        assert_eq!(variation.active, 0);

        assert_eq!(variation.variants.len(), 2);

        let delete_4 = &variation.variants[0];
        let delete_5 = &variation.variants[1];

        assert_eq!(delete_4.name, "delete_4");
        assert_eq!(
            delete_4.code,
            vec![
                "  if k <? k' then delete k l",
                "  else if k' <? k then delete k r",
                "  else join l r",
            ]
        );

        assert_eq!(delete_5.name, "delete_5");
        assert_eq!(
            delete_5.code,
            vec![
                "  if k' <? k then T (delete k l) k' v' r",
                "  else if k <? k' then T l k' v' (delete k r)",
                "  else join l r",
            ]
        );

        assert_eq!(line, 17);
    }

    #[test]
    fn test_tags() {
        let input = r#"[new, easy]"#;
        let result = Parser::parse(Rule::tags, input).unwrap().next().unwrap();
        let tags = result
            .into_inner()
            .map(|pair| pair.as_str().to_string())
            .collect::<Vec<String>>();

        assert_eq!(tags, vec!["new".to_string(), "easy".to_string()]);
    }

    #[test]
    fn test_variation_header() {
        let input = r#"    (*! insert [new, easy] *)"#;
        let result = Parser::parse(Rule::variation_header, input)
            .unwrap()
            .next()
            .unwrap();
        let (name, tags, indent) = parse_variation_header(result);

        assert_eq!(name, Some("insert".to_string()));
        assert_eq!(tags, vec!["new".to_string(), "easy".to_string()]);
        assert_eq!(indent, "    ");
    }

    #[test]
    fn test_code() {
        let result = Parser::parse(
            Rule::code,
            r#"Fixpoint delete (k: nat) (t: Tree) :=
  match t with
  | E => E
  | T l k' v' r =>
  (*! *)
  if k <? k' then T (delete k l) k' v' r
  else if k' <? k then T l k' v' (delete k r)
  else join l r
  (*!! delete_4 *)
  (*!
  if k <? k' then delete k l
  else if k' <? k then delete k r
  else join l r
  *)
  (*!! delete_5 *)
  (*!
  if k' <? k then T (delete k l) k' v' r
  else if k <? k' then T l k' v' (delete k r)
  else join l r
  *)
  (* !*)
  end."#,
        )
        .unwrap()
        .next()
        .unwrap();

        let result = parse_code(result.as_str()).unwrap();

        let s1 = &result[0];
        if let SpanContent::Line(c) = &s1.content {
            assert_eq!(
                c,
                r#"Fixpoint delete (k: nat) (t: Tree) :=
  match t with
  | E => E
  | T l k' v' r =>
"#
            );
        } else {
            panic!("unexpected span content {:?}", s1.content);
        }

        let s2 = &result[1];

        if let SpanContent::Variation(v) = &s2.content {
            assert_eq!(v.name, None);
            assert_eq!(v.tags, vec![] as Vec<String>);
            assert_eq!(v.active, 0);
            assert_eq!(
                v.base.0,
                vec![
                    "  if k <? k' then T (delete k l) k' v' r",
                    "  else if k' <? k then T l k' v' (delete k r)",
                    "  else join l r",
                ]
            );
            assert_eq!(v.variants.len(), 2);
            assert_eq!(v.variants[0].name, "delete_4");
            assert_eq!(
                v.variants[0].code,
                vec![
                    "  if k <? k' then delete k l",
                    "  else if k' <? k then delete k r",
                    "  else join l r",
                ]
            );
            assert_eq!(v.variants[1].name, "delete_5");
            assert_eq!(
                v.variants[1].code,
                vec![
                    "  if k' <? k then T (delete k l) k' v' r",
                    "  else if k <? k' then T l k' v' (delete k r)",
                    "  else join l r",
                ]
            );
        } else {
            panic!("unexpected span content {:?}", s2.content);
        }
    }

    #[test]
    fn test_parse_code_roundtrip() {
        let code = fs::read_to_string("test/coq/BST.v").unwrap();
        let spans = parse_code(&code).unwrap();
        let code = Code::new(
            crate::languages::Language::Coq,
            spans.clone(),
            PathBuf::from("test/coq/BST2.v"),
        );
        let code_as_str = code.to_string();
        let spans2 = parse_code(&code_as_str).unwrap();

        assert_eq!(spans.len(), spans2.len());
        for (span, span2) in spans.iter().zip(spans2.iter()) {
            assert_eq!(span, span2);
        }
    }

    #[test]
    fn test_alternative_mutation_marker() {
        let result = parse_code(
            r#"Fixpoint delete (k: nat) (t: Tree) :=
    match t with
    | E => E
    | T l k' v' r =>
        (*| *)
        if k <? k' then T (delete k l) k' v' r
        else if k' <? k then T l k' v' (delete k r)
        else join l r
        (*|| delete_4 *)
        (*|
        if k <? k' then delete k l
        else if k' <? k then delete k r
        else join l r
        *)
        (*|| delete_5 *)
        (*|
        if k' <? k then T (delete k l) k' v' r
        else if k <? k' then T l k' v' (delete k r)
        else join l r
        *)
        (* |*)
end."#,
        )
        .unwrap();

        if let SpanContent::Line(c) = &result[0].content {
            assert_eq!(
                c,
                r#"Fixpoint delete (k: nat) (t: Tree) :=
    match t with
    | E => E
    | T l k' v' r =>
"#
            );
        } else {
            panic!("unexpected span content {:?}", result[0].content);
        }

        if let SpanContent::Variation(v) = &result[1].content {
            assert_eq!(v.name, None);
            assert_eq!(v.tags, vec![] as Vec<String>);
            assert_eq!(v.active, 0);
            assert_eq!(
                v.base.0,
                vec![
                    "        if k <? k' then T (delete k l) k' v' r",
                    "        else if k' <? k then T l k' v' (delete k r)",
                    "        else join l r",
                ]
            );
            assert_eq!(v.variants.len(), 2);
            assert_eq!(v.variants[0].name, "delete_4");
            assert_eq!(
                v.variants[0].code,
                vec![
                    "        if k <? k' then delete k l",
                    "        else if k' <? k then delete k r",
                    "        else join l r",
                ]
            );
            assert_eq!(v.variants[1].name, "delete_5");
            assert_eq!(
                v.variants[1].code,
                vec![
                    "        if k' <? k then T (delete k l) k' v' r",
                    "        else if k <? k' then T l k' v' (delete k r)",
                    "        else join l r",
                ]
            );
        } else {
            panic!("unexpected span content {:?}", result[1].content);
        }
    }
}
