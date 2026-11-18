//! A small s-expression reader for KiCad files.

use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    Atom(String),
    List(Vec<SExpr>),
}

pub fn parse(text: &str) -> Result<SExpr> {
    let mut parser = Parser {
        chars: text.chars().peekable(),
    };
    parser.skip();
    let expr = parser.expr()?;
    parser.skip();
    if parser.chars.peek().is_some() {
        bail!("trailing data after the KiCad s-expression");
    }
    Ok(expr)
}

struct Parser<I: Iterator<Item = char>> {
    chars: std::iter::Peekable<I>,
}

impl<I: Iterator<Item = char>> Parser<I> {
    fn skip(&mut self) {
        while let Some(c) = self.chars.peek().copied() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn expr(&mut self) -> Result<SExpr> {
        self.skip();
        match self.chars.peek().copied() {
            Some('(') => {
                self.chars.next();
                let mut items = Vec::new();
                loop {
                    self.skip();
                    match self.chars.peek().copied() {
                        None => bail!("unclosed s-expression"),
                        Some(')') => {
                            self.chars.next();
                            break;
                        }
                        Some(_) => items.push(self.expr()?),
                    }
                }
                Ok(SExpr::List(items))
            }
            Some('"') => Ok(SExpr::Atom(self.quoted()?)),
            Some(_) => Ok(SExpr::Atom(self.bare())),
            None => bail!("empty s-expression"),
        }
    }

    fn quoted(&mut self) -> Result<String> {
        self.chars.next();
        let mut out = String::new();
        while let Some(c) = self.chars.next() {
            match c {
                '"' => return Ok(out),
                '\\' => {
                    if let Some(next) = self.chars.next() {
                        out.push(next);
                    }
                }
                _ => out.push(c),
            }
        }
        bail!("unclosed string")
    }

    fn bare(&mut self) -> String {
        let mut out = String::new();
        while let Some(c) = self.chars.peek().copied() {
            if c.is_whitespace() || c == '(' || c == ')' {
                break;
            }
            out.push(c);
            self.chars.next();
        }
        out
    }
}

pub fn head<'a>(expr: &'a SExpr) -> Option<&'a str> {
    match expr {
        SExpr::List(items) => items.first().and_then(|item| match item {
            SExpr::Atom(text) => Some(text.as_str()),
            SExpr::List(_) => None,
        }),
        SExpr::Atom(_) => None,
    }
}

pub fn atom_at<'a>(expr: &'a SExpr, index: usize) -> Option<&'a str> {
    match expr {
        SExpr::List(items) => items.get(index).and_then(|item| match item {
            SExpr::Atom(text) => Some(text.as_str()),
            SExpr::List(_) => None,
        }),
        SExpr::Atom(_) => None,
    }
}

pub fn children(expr: &SExpr) -> &[SExpr] {
    match expr {
        SExpr::List(items) => items,
        SExpr::Atom(_) => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_quoted_atom_and_rejects_trailing_junk() {
        let expr = parse(r#"(symbol "Device:R" (property "LCSC" "C1"))"#).unwrap();
        let SExpr::List(items) = expr else {
            panic!("list");
        };
        assert_eq!(items[1], SExpr::Atom("Device:R".into()));
        assert!(parse("(a) trailing").is_err());
    }
}
