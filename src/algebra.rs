use std::{collections::HashMap, fmt::Display, iter::Peekable, str::Chars};

type Id = String;
type Tag = String;

#[derive(Debug, PartialEq)]
pub(crate) enum Expr {
    Sum(Box<Expr>, Box<Expr>),
    Product(Box<Expr>, Box<Expr>),
    USum(Tag),
    UProduct(Tag),
    Id(Id),
}

impl Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::Sum(lhs, rhs) => write!(f, "({} + {})", lhs, rhs),
            Expr::Product(lhs, rhs) => write!(f, "({} * {})", lhs, rhs),
            Expr::USum(tag) => write!(f, "+{}", tag),
            Expr::UProduct(tag) => write!(f, "*{}", tag),
            Expr::Id(id) => write!(f, "{}", id),
        }
    }
}

impl Expr {
    /// Converts the expression into a sum of products
    /// @self: The expression to convert
    /// @tag_map: A map from tag to a list of ids that have that tag
    /// @variation_map: A map from variation ids to a list of ids that are variants within the variation
    /// @variant_list: A complete list of all variant ids
    /// @return: A list of lists of ids that represent the sum of products
    pub(crate) fn into_sum_of_products(
        &self,
        tag_map: HashMap<String, Vec<String>>,
        variation_map: HashMap<String, Vec<String>>,
        variant_list: Vec<String>,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        // Distributes tags +t into (t1 + t2 + ... + tn) and *t into (t1 * t2 * ... * tn)
        let tagless_expr = self.distribute_tags(&tag_map);
        // Distributes variations v into (v1 + v2 + ... + vn)
        let variation_distributed =
            tagless_expr.distribute_variations(&variation_map, &variant_list);
        // Check that all the variants in the expression are in the variant list
        let mut variants = vec![];
        variation_distributed.collect_variants(&mut variants);
        for variant in variants.iter() {
            if !variant_list.contains(variant) {
                anyhow::bail!("Variant {} is not in the variant list", variant);
            }
        }
        let variation_distributed = variation_distributed.distribute();

        Ok(variation_distributed)
    }

    fn collect_variants(&self, variants: &mut Vec<String>) {
        match self {
            Expr::Sum(lhs, rhs) => {
                lhs.collect_variants(variants);
                rhs.collect_variants(variants);
            }
            Expr::Product(lhs, rhs) => {
                lhs.collect_variants(variants);
                rhs.collect_variants(variants);
            }
            Expr::USum(_) | Expr::UProduct(_) => {
                unreachable!("Unary expressions are elimiated in [distributed_tags] phase")
            }
            Expr::Id(id) => {
                variants.push(id.clone());
            }
        }
    }

    fn distribute_tags(&self, tag_map: &HashMap<String, Vec<String>>) -> Expr {
        match self {
            Expr::Sum(lhs, rhs) => {
                let lhs = lhs.distribute_tags(tag_map);
                let rhs = rhs.distribute_tags(tag_map);
                Expr::Sum(Box::new(lhs), Box::new(rhs))
            }
            Expr::Product(lhs, rhs) => {
                let lhs = lhs.distribute_tags(tag_map);
                let rhs = rhs.distribute_tags(tag_map);
                Expr::Product(Box::new(lhs), Box::new(rhs))
            }
            Expr::USum(tag) => {
                let ids = vec![];
                let ids = tag_map.get(tag).unwrap_or(&ids);
                let mut sum = Expr::Id(ids[0].clone());
                for id in ids.iter().skip(1) {
                    sum = Expr::Sum(Box::new(sum), Box::new(Expr::Id(id.clone())));
                }
                sum
            }
            Expr::UProduct(tag) => {
                let ids = vec![];
                let ids = tag_map.get(tag).unwrap_or(&ids);
                let mut product = Expr::Id(ids[0].clone());
                for id in ids.iter().skip(1) {
                    product = Expr::Product(Box::new(product), Box::new(Expr::Id(id.clone())));
                }
                product
            }
            Expr::Id(id) => Expr::Id(id.clone()),
        }
    }

    fn distribute_variations(
        &self,
        variation_map: &HashMap<String, Vec<String>>,
        variant_list: &Vec<String>,
    ) -> Expr {
        match self {
            Expr::Sum(lhs, rhs) => {
                let lhs = lhs.distribute_variations(variation_map, variant_list);
                let rhs = rhs.distribute_variations(variation_map, variant_list);
                Expr::Sum(Box::new(lhs), Box::new(rhs))
            }
            Expr::Product(lhs, rhs) => {
                let lhs = lhs.distribute_variations(variation_map, variant_list);
                let rhs = rhs.distribute_variations(variation_map, variant_list);
                Expr::Product(Box::new(lhs), Box::new(rhs))
            }
            Expr::USum(_) | Expr::UProduct(_) => {
                unreachable!("Unary expressions are elimiated in [distributed_tags] phase")
            }
            Expr::Id(id) => match variation_map.get(id) {
                Some(ids) => {
                    let mut sum = Expr::Id(ids[0].clone());
                    for id in ids.iter().skip(1) {
                        sum = Expr::Sum(Box::new(sum), Box::new(Expr::Id(id.clone())));
                    }
                    sum
                }
                None => Expr::Id(id.clone()),
            },
        }
    }

    fn distribute(&self) -> Vec<Vec<String>> {
        match self {
            Expr::Product(lhs, rhs) => {
                let lhs = lhs.distribute();
                let rhs = rhs.distribute();
                let mut result = vec![];
                for l in lhs.iter() {
                    for r in rhs.iter() {
                        let mut sum = l.clone();
                        sum.extend(r.iter().cloned());
                        result.push(sum);
                    }
                }
                result
            }
            Expr::Sum(lhs, rhs) => {
                let mut lhs = lhs.distribute();
                let rhs = rhs.distribute();
                lhs.extend(rhs);
                lhs
            }
            Expr::USum(_) | Expr::UProduct(_) => {
                unreachable!("Unary expressions are elimiated in [distributed_tags] phase")
            }
            Expr::Id(id) => vec![vec![id.clone()]],
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
enum Token {
    Plus,
    Star,
    OpenParen,
    CloseParen,
    Identifier(String),
}

struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        while let Some(&c) = self.input.peek() {
            match c {
                '+' => {
                    self.input.next();
                    return Some(Token::Plus);
                }
                '*' => {
                    self.input.next();
                    return Some(Token::Star);
                }
                '(' => {
                    self.input.next();
                    return Some(Token::OpenParen);
                }
                ')' => {
                    self.input.next();
                    return Some(Token::CloseParen);
                }
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => {
                    let mut id = String::new();
                    while let Some(&ch) = self.input.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            id.push(ch);
                            self.input.next();
                        } else {
                            break;
                        }
                    }
                    return Some(Token::Identifier(id));
                }
                ' ' | '\t' | '\n' => {
                    self.input.next(); // Skip whitespace
                }
                _ => panic!("Unexpected character: {}", c),
            }
        }
        None
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current_token: Option<Token>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token();
        Self {
            lexer,
            current_token,
        }
    }

    fn consume(&mut self) {
        self.current_token = self.lexer.next_token();
    }

    fn parse(&mut self) -> Expr {
        self.parse_sum()
    }

    fn parse_sum(&mut self) -> Expr {
        let mut left = self.parse_product();

        while let Some(Token::Plus) = self.current_token {
            self.consume();
            let right = self.parse_product();
            left = Expr::Sum(Box::new(left), Box::new(right));
        }

        left
    }

    fn parse_product(&mut self) -> Expr {
        let mut left = self.parse_primary();

        while let Some(Token::Star) = self.current_token {
            self.consume();
            let right = self.parse_primary();
            left = Expr::Product(Box::new(left), Box::new(right));
        }

        left
    }

    fn parse_primary(&mut self) -> Expr {
        match self.current_token.clone() {
            Some(Token::Plus) => {
                self.consume();
                let tag = match self.current_token.clone() {
                    Some(Token::Identifier(id)) => {
                        self.consume();
                        id
                    }
                    _ => panic!("Expected identifier after unary plus"),
                };
                Expr::USum(tag)
            }
            Some(Token::Star) => {
                self.consume();
                let tag = match self.current_token.clone() {
                    Some(Token::Identifier(id)) => {
                        self.consume();
                        id
                    }
                    _ => panic!("Expected identifier after unary star"),
                };
                Expr::UProduct(tag)
            }
            Some(Token::Identifier(id)) => {
                self.consume();
                Expr::Id(id)
            }
            Some(Token::OpenParen) => {
                self.consume();
                let expr = self.parse();
                if let Some(Token::CloseParen) = self.current_token {
                    self.consume();
                    expr
                } else {
                    panic!("Expected closing parenthesis");
                }
            }
            _ => panic!("Unexpected token: {:?}", self.current_token),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_expr() {
        let input = "a + b * c";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        assert_eq!(
            expr,
            Expr::Sum(
                Box::new(Expr::Id("a".to_string())),
                Box::new(Expr::Product(
                    Box::new(Expr::Id("b".to_string())),
                    Box::new(Expr::Id("c".to_string()))
                ))
            )
        );
    }

    #[test]
    fn test_nested_expr() {
        let input = "a + ((b * c) + d)";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        assert_eq!(
            expr,
            Expr::Sum(
                Box::new(Expr::Id("a".to_string())),
                Box::new(Expr::Sum(
                    Box::new(Expr::Product(
                        Box::new(Expr::Id("b".to_string())),
                        Box::new(Expr::Id("c".to_string()))
                    )),
                    Box::new(Expr::Id("d".to_string()))
                ))
            )
        );
    }

    #[test]
    fn test_complex_names() {
        let input = "a1 + b_2 * c3";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        assert_eq!(
            expr,
            Expr::Sum(
                Box::new(Expr::Id("a1".to_string())),
                Box::new(Expr::Product(
                    Box::new(Expr::Id("b_2".to_string())),
                    Box::new(Expr::Id("c3".to_string()))
                ))
            )
        );
    }

    #[test]
    fn test_unary_expr() {
        let input = "+easy * insert";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        assert_eq!(
            expr,
            Expr::Product(
                Box::new(Expr::USum("easy".to_string())),
                Box::new(Expr::Id("insert".to_string()))
            )
        );
    }

    #[test]
    fn test_unary_expr_paren() {
        let input = "+easy * (insert + delete)";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        assert_eq!(
            expr,
            Expr::Product(
                Box::new(Expr::USum("easy".to_string())),
                Box::new(Expr::Sum(
                    Box::new(Expr::Id("insert".to_string())),
                    Box::new(Expr::Id("delete".to_string()))
                ))
            )
        );
    }

    #[test]
    fn test_distribute_tags() {
        let input = "+easy * (insert + delete)";
        let mut parser = Parser::new(input);
        let expr = parser.parse();
        let tag_map = vec![("easy".to_string(), vec!["a".to_string(), "b".to_string()])]
            .into_iter()
            .collect();
        let variation_map = vec![
            (
                "insert".to_string(),
                vec!["insert_1".to_string(), "insert_2".to_string()],
            ),
            (
                "delete".to_string(),
                vec!["delete_1".to_string(), "delete_2".to_string()],
            ),
        ]
        .into_iter()
        .collect();

        let variant_list = vec![
            "a".to_string(),
            "b".to_string(),
            "insert_1".to_string(),
            "insert_2".to_string(),
            "delete_1".to_string(),
            "delete_2".to_string(),
        ];
        let sum_of_products = expr
            .into_sum_of_products(tag_map, variation_map, variant_list)
            .unwrap();
        assert_eq!(
            sum_of_products,
            vec![
                vec!["a".to_string(), "insert_1".to_string()],
                vec!["a".to_string(), "insert_2".to_string()],
                vec!["a".to_string(), "delete_1".to_string()],
                vec!["a".to_string(), "delete_2".to_string()],
                vec!["b".to_string(), "insert_1".to_string()],
                vec!["b".to_string(), "insert_2".to_string()],
                vec!["b".to_string(), "delete_1".to_string()],
                vec!["b".to_string(), "delete_2".to_string()],
            ]
        );
    }
}
