use std::collections::HashMap;

use pest::Parser as _;
use pest_derive::Parser;

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

fn parse(input: &str) -> Result<AST, pest::error::Error<Rule>> {
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
            let symbol = pair.peek().and_then(|p| Some(parse_string(p)));
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
                return Err(anyhow::anyhow!("Expected a boolean"));
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
