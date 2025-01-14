use pest::Parser as _;
use pest_derive::Parser;

use crate::code::Span;
use crate::variation::{Variant, Variation};

#[derive(Parser)]
#[grammar = "syntax/comment.pest"]
pub(crate) struct Parser;

pub(crate) fn parse_code(input: &str) -> anyhow::Result<Vec<Span>> {
    let mut pairs = Parser::parse(Rule::code, input)?;
    let pairs = pairs.next().unwrap().into_inner();
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
        Rule::text => {
            spans.push(Span::constant(pair.as_str().to_string(), line));
            pair.as_str().lines().count()
        }
        Rule::mutation => {
            let (variation, current_lines) = parse_variation(pair.into_inner().next().unwrap());
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
    let base = parse_base(pairs.next().unwrap());
    let mut variants = vec![];
    for pair in pairs {
        if pair.as_rule() == Rule::end {
            break;
        }
        variants.push(parse_variant(pair));
    }

    // only one of the variants or the base can be active
    let actives = variants.iter().filter(|(_, active)| *active).count();
    let active = if base.1 {
        assert_eq!(actives, 0);
        0
    } else {
        assert_eq!(actives, 1);
        variants.iter().position(|(_, active)| *active).unwrap() + 1
    };

    let (name, tags) = parse_variation_header(header);

    let mut lines = 0;
    // Begin marker (*! *)
    lines += 1;
    // Base code
    lines += base.0.lines().count();
    // Variant codes
    for (variant, _) in &variants {
        // Begin marker (*!! *)
        lines += 1;
        // Variant code
        lines += variant.code.lines().count();
    }
    // End marker (* !*)
    lines += 1;
    // Inline markers for the passive variants
    lines += variants.len() * 2;

    (
        Variation {
            name,
            tags,
            base: base.0,
            variants: variants.into_iter().map(|(v, _)| v).collect(),
            active,
        },
        lines,
    )
}

fn parse_variation_header(pair: pest::iterators::Pair<Rule>) -> (Option<String>, Vec<String>) {
    let mut pairs = pair.into_inner();
    let _begin_marker = pairs.next().unwrap();
    let maybe_name = pairs.peek().unwrap();
    let name = match maybe_name.as_rule() {
        Rule::identifier => {
            // Move the iterator
            pairs.next().unwrap();
            Some(maybe_name.as_str().to_string())
        }
        Rule::comment_end | Rule::tags => None,
        p => unreachable!("Unexpected rule {:?}", p),
    };

    let maybe_tags = pairs.peek().unwrap();
    let tags: Vec<String> = match maybe_tags.as_rule() {
        Rule::tags => {
            // Move the iterator
            pairs.next().unwrap();
            maybe_tags
                .into_inner()
                .map(|pair| pair.as_str().to_string())
                .collect()
        }
        Rule::comment_end => vec![],
        p => unreachable!("Unexpected rule {:?}", p),
    };

    let _end_marker = pairs.next().unwrap();

    (name, tags)
}

fn parse_base(pair: pest::iterators::Pair<Rule>) -> (String, bool) {
    let mut pairs = pair.into_inner();
    let body = pairs.next().unwrap();

    parse_variant_body(body)
}

fn parse_variant(pair: pest::iterators::Pair<Rule>) -> (crate::variation::Variant, bool) {
    let mut pairs = pair.into_inner();
    let header = pairs.next().unwrap();
    let body = pairs.next().unwrap();

    let (code, is_active) = parse_variant_body(body);
    (
        Variant {
            name: parse_variant_header(header),
            code,
        },
        is_active,
    )
}

fn parse_variant_header(pair: pest::iterators::Pair<Rule>) -> String {
    let mut pairs = pair.into_inner();
    let begin_marker = pairs.next().unwrap();
    let name = pairs.next().unwrap();
    let end_marker = pairs.next().unwrap();

    name.as_str().to_string()
}

fn parse_variant_body(pair: pest::iterators::Pair<Rule>) -> (String, bool) {
    let body = pair.into_inner().next().unwrap();
    match body.as_rule() {
        Rule::inactive_variant_body => {
            let mut pairs = body.into_inner();
            let begin_marker = pairs.next().unwrap();
            let body = pairs.next().unwrap();
            let end_marker = pairs.next().unwrap();

            (body.as_str().to_string(), false)
        }
        Rule::active_variant_body => {
            let mut pairs = body.into_inner();
            let body = pairs.next().unwrap();
            (body.as_str().to_string(), true)
        }
        p => unreachable!("unexpected rule {:?}", p),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, result};

    use crate::code::Code;

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
        let mut result = result.into_inner();
        let begin_marker = result.next().unwrap();
        let name = result.next().unwrap();
        let end_marker = result.next().unwrap();

        assert_eq!(begin_marker.as_str(), "(*!");
        assert_eq!(name.as_str(), "delete_4");
        assert_eq!(end_marker.as_str(), "*)");
    }

    #[test]
    fn test_parse_variation_header_noid() {
        let result = Parser::parse(Rule::variation_header, r#"(*! *)"#)
            .unwrap()
            .next()
            .unwrap();
        let mut result = result.into_inner();
        let begin_marker = result.next().unwrap();
        let end_marker = result.next().unwrap();

        assert_eq!(begin_marker.as_str(), "(*!");
        assert_eq!(end_marker.as_str(), "*)");
        assert!(result.next().is_none());
    }

    #[test]
    fn test_variation_end() {
        let result = Parser::parse(Rule::end, r#"(* !*)"#)
            .unwrap()
            .next()
            .unwrap();

        assert_eq!(result.as_str(), "(* !*)");
    }

    #[test]
    fn test_variation_end_whitespace() {
        let result = Parser::parse(Rule::end, r#"(*! *)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_variation_end_whitespace2() {
        let result = Parser::parse(Rule::end, r#"(* ! *)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_variation_base() {
        let result = Parser::parse(
            Rule::base,
            r#"
if k <? k' then T (delete k l) k' v' r
else if k' <? k then T l k' v' (delete k r)
else join l r"#,
        )
        .unwrap()
        .next()
        .unwrap();

        assert_eq!(
            result.as_str(),
            r#"
if k <? k' then T (delete k l) k' v' r
else if k' <? k then T l k' v' (delete k r)
else join l r"#
        );
    }

    #[test]
    fn test_variant_header() {
        let result = Parser::parse(Rule::variant_header, r#"(*!! delete_4 *)"#)
            .unwrap()
            .next()
            .unwrap();
        let mut result = result.into_inner();
        let begin_marker = result.next().unwrap();
        let name = result.next().unwrap();
        let end_marker = result.next().unwrap();

        assert_eq!(begin_marker.as_str(), "(*!!");
        assert_eq!(name.as_str(), "delete_4");
        assert_eq!(end_marker.as_str(), "*)");
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
else join l r *)
"#,
        )
        .unwrap()
        .next()
        .unwrap()
        .into_inner();

        let mut result = result.next().unwrap().into_inner();

        assert_eq!(result.next().unwrap().as_str(), "(*!");
        assert_eq!(
            result.next().unwrap().as_str(),
            r#"if k <? k' then delete k l
else if k' <? k then delete k r
else join l r"#
        );
        assert_eq!(result.next().unwrap().as_str(), "*)");
    }

    #[test]
    fn test_variant() {
        let result = Parser::parse(
            Rule::variant,
            r#"(*!! delete_4 *)
(*!
if k <? k' then delete k l
else if k' <? k then delete k r
else join l r
*)"#,
        )
        .unwrap()
        .next()
        .unwrap();

        assert!(result.as_rule() == Rule::variant);
        let mut result = result.into_inner();
        let header = result.next().unwrap();
        let body = result.next().unwrap();

        assert_eq!(header.as_str(), "(*!! delete_4 *)");
        assert_eq!(header.as_rule(), Rule::variant_header);
        assert_eq!(
            body.as_str(),
            r#"(*!
if k <? k' then delete k l
else if k' <? k then delete k r
else join l r
*)"#
        );
        assert_eq!(body.as_rule(), Rule::variant_body);
    }

    #[test]
    fn test_variation() {
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

        assert!(result.as_rule() == Rule::variation);
        let mut result = result.into_inner();
        let header = result.next().unwrap();
        let base = result.next().unwrap();
        assert_eq!(
            base.as_str(),
            r#"if k <? k' then T (delete k l) k' v' r
  else if k' <? k then T l k' v' (delete k r)
  else join l r"#
        );
        let delete_4 = result.next().unwrap();
        let delete_5 = result.next().unwrap();
        let end = result.next().unwrap();

        assert_eq!(header.as_str(), "(*! *)");
        assert_eq!(base.as_rule(), Rule::base);
        assert_eq!(delete_4.as_rule(), Rule::variant);
        assert_eq!(delete_5.as_rule(), Rule::variant);
        assert_eq!(end.as_rule(), Rule::end);
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

        assert!(result.as_rule() == Rule::code);
        let mut result = result.into_inner();
        let s1 = result.next().unwrap();
        assert_eq!(s1.as_rule(), Rule::text);
        assert_eq!(
            s1.as_str(),
            r#"Fixpoint delete (k: nat) (t: Tree) :=
  match t with
  | E => E
  | T l k' v' r =>"#
        );

        let s2 = result.next().unwrap();
        assert_eq!(s2.as_rule(), Rule::mutation);
        assert_eq!(
            s2.as_str(),
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
  (* !*)"#
        );
    }

    #[test]
    fn test_parse_code_roundtrip() {
        let code = fs::read_to_string("test/BST.v").unwrap();
        let spans = parse_code(&code).unwrap();
        let code = Code::new(
            crate::languages::Language::Coq,
            spans.clone(),
            PathBuf::from("test/BST2.v"),
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
        let result = Parser::parse(
            Rule::code,
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
        .unwrap()
        .next()
        .unwrap();

        assert!(result.as_rule() == Rule::code);
        let mut result = result.into_inner();
        let s1 = result.next().unwrap();
        assert_eq!(s1.as_rule(), Rule::text);
        assert_eq!(
            s1.as_str(),
            r#"Fixpoint delete (k: nat) (t: Tree) :=
  match t with
  | E => E
  | T l k' v' r =>"#
        );

        let s2 = result.next().unwrap();
        assert_eq!(s2.as_rule(), Rule::mutation);
        assert_eq!(
            s2.as_str(),
            r#"(*| *)
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
  (* |*)"#
        );
    }
}
