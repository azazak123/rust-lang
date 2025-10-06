// src/parser.rs
use crate::expr::{BinaryOp, Expr, UnaryOp};
use crate::token::TokenType;

pub struct Parser {
    tokens: Vec<TokenType>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<TokenType>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&TokenType> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<TokenType> {
        if self.pos < self.tokens.len() {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(token)
        } else {
            None
        }
    }

    fn check(&self, expected: &TokenType) -> bool {
        if let Some(token) = self.peek() {
            std::mem::discriminant(token) == std::mem::discriminant(expected)
        } else {
            false
        }
    }

    pub fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut expr = self.parse_and()?;

        while self.check(&TokenType::Or) {
            self.advance();
            let right = self.parse_and()?;
            expr = Expr::Binary(Box::new(expr), BinaryOp::Or, Box::new(right));
        }

        Some(expr)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut expr = self.parse_equality()?;

        while self.check(&TokenType::And) {
            self.advance();
            let right = self.parse_equality()?;
            expr = Expr::Binary(Box::new(expr), BinaryOp::And, Box::new(right));
        }

        Some(expr)
    }

    fn parse_equality(&mut self) -> Option<Expr> {
        let mut expr = self.parse_comparison()?;

        while matches!(
            self.peek(),
            Some(TokenType::EqualEqual) | Some(TokenType::BangEqual)
        ) {
            let op = match self.advance()? {
                TokenType::EqualEqual => BinaryOp::EqualEqual,
                TokenType::BangEqual => BinaryOp::BangEqual,
                _ => return None,
            };
            let right = self.parse_comparison()?;
            expr = Expr::Binary(Box::new(expr), op, Box::new(right));
        }

        Some(expr)
    }

    fn parse_comparison(&mut self) -> Option<Expr> {
        let mut expr = self.parse_addition()?;

        while matches!(
            self.peek(),
            Some(TokenType::Less)
                | Some(TokenType::LessEqual)
                | Some(TokenType::Greater)
                | Some(TokenType::GreaterEqual)
        ) {
            let op = match self.advance()? {
                TokenType::Less => BinaryOp::Less,
                TokenType::LessEqual => BinaryOp::LessEqual,
                TokenType::Greater => BinaryOp::Greater,
                TokenType::GreaterEqual => BinaryOp::GreaterEqual,
                _ => return None,
            };
            let right = self.parse_addition()?;
            expr = Expr::Binary(Box::new(expr), op, Box::new(right));
        }

        Some(expr)
    }

    fn parse_addition(&mut self) -> Option<Expr> {
        let mut expr = self.parse_multiplication()?;

        while matches!(self.peek(), Some(TokenType::Plus) | Some(TokenType::Minus)) {
            let op = match self.advance()? {
                TokenType::Plus => BinaryOp::Plus,
                TokenType::Minus => BinaryOp::Minus,
                _ => return None,
            };
            let right = self.parse_multiplication()?;
            expr = Expr::Binary(Box::new(expr), op, Box::new(right));
        }

        Some(expr)
    }

    fn parse_multiplication(&mut self) -> Option<Expr> {
        let mut expr = self.parse_unary()?;

        while matches!(
            self.peek(),
            Some(TokenType::Star) | Some(TokenType::Slash) | Some(TokenType::Percent)
        ) {
            let op = match self.advance()? {
                TokenType::Star => BinaryOp::Star,
                TokenType::Slash => BinaryOp::Slash,
                TokenType::Percent => BinaryOp::Modulo,
                _ => return None,
            };
            let right = self.parse_unary()?;
            expr = Expr::Binary(Box::new(expr), op, Box::new(right));
        }

        Some(expr)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        if matches!(self.peek(), Some(TokenType::Bang) | Some(TokenType::Minus)) {
            let op = match self.advance()? {
                TokenType::Bang => UnaryOp::Bang,
                TokenType::Minus => UnaryOp::Minus,
                _ => return None,
            };
            let expr = self.parse_unary()?;
            return Some(Expr::Unary(op, Box::new(expr)));
        }

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        match self.peek()? {
            TokenType::Number(n) => {
                let n = *n;
                self.advance();
                Some(Expr::Number(n))
            }
            TokenType::StringLit(s) => {
                let s = s.clone();
                self.advance();
                Some(Expr::String(s))
            }
            TokenType::True => {
                self.advance();
                Some(Expr::Bool(true))
            }
            TokenType::False => {
                self.advance();
                Some(Expr::Bool(false))
            }
            TokenType::Nil => {
                self.advance();
                Some(Expr::Nil)
            }
            TokenType::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Some(Expr::Var(name))
            }
            TokenType::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                if !self.check(&TokenType::RightParen) {
                    return None;
                }
                self.advance();
                Some(expr)
            }
            TokenType::LeftBracket => {
                self.advance();
                self.parse_array_or_range()
            }
            TokenType::Map => {
                self.advance();
                self.parse_map()
            }
            TokenType::Filter => {
                self.advance();
                self.parse_filter()
            }
            TokenType::Scanl => {
                self.advance();
                self.parse_scanl()
            }
            TokenType::Foldl => {
                self.advance();
                self.parse_foldl()
            }
            _ => None,
        }
    }

    fn parse_array_or_range(&mut self) -> Option<Expr> {
        if self.check(&TokenType::RightBracket) {
            self.advance();
            return Some(Expr::Array(vec![]));
        }

        let first = self.parse_expr()?;

        if self.check(&TokenType::DotDot) {
            self.advance();
            let end = self.parse_expr()?;
            if !self.check(&TokenType::RightBracket) {
                return None;
            }
            self.advance();
            return Some(Expr::Range(Box::new(first), Box::new(end)));
        }

        let mut elements = vec![first];
        while self.check(&TokenType::Comma) {
            self.advance();
            if self.check(&TokenType::RightBracket) {
                break;
            }
            elements.push(self.parse_expr()?);
        }

        if !self.check(&TokenType::RightBracket) {
            return None;
        }
        self.advance();
        Some(Expr::Array(elements))
    }

    fn parse_map(&mut self) -> Option<Expr> {
        let TokenType::Identifier(var) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::In) {
            return None;
        }
        self.advance();

        let arr = self.parse_until_brace()?;

        if !self.check(&TokenType::LeftBrace) {
            return None;
        }
        self.advance();

        let body = self.parse_until_close_brace()?;

        if !self.check(&TokenType::RightBrace) {
            return None;
        }
        self.advance();

        Some(Expr::MapExpr(var, Box::new(arr), Box::new(body)))
    }

    fn parse_filter(&mut self) -> Option<Expr> {
        let TokenType::Identifier(var) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::In) {
            return None;
        }
        self.advance();

        let arr = self.parse_until_brace()?;

        if !self.check(&TokenType::LeftBrace) {
            return None;
        }
        self.advance();

        let body = self.parse_until_close_brace()?;

        if !self.check(&TokenType::RightBrace) {
            return None;
        }
        self.advance();

        Some(Expr::FilterExpr(var, Box::new(arr), Box::new(body)))
    }

    fn parse_scanl(&mut self) -> Option<Expr> {
        let TokenType::Identifier(acc_name) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::Comma) {
            return None;
        }
        self.advance();

        let init = self.parse_until_comma()?;

        if !self.check(&TokenType::Comma) {
            return None;
        }
        self.advance();

        let TokenType::Identifier(var) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::In) {
            return None;
        }
        self.advance();

        let arr = self.parse_until_brace()?;

        if !self.check(&TokenType::LeftBrace) {
            return None;
        }
        self.advance();

        let body = self.parse_until_close_brace()?;

        if !self.check(&TokenType::RightBrace) {
            return None;
        }
        self.advance();

        Some(Expr::ScanlExpr(
            acc_name,
            Box::new(init),
            var,
            Box::new(arr),
            Box::new(body),
        ))
    }

    fn parse_foldl(&mut self) -> Option<Expr> {
        let TokenType::Identifier(acc_name) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::Comma) {
            return None;
        }
        self.advance();

        let init = self.parse_until_comma()?;

        if !self.check(&TokenType::Comma) {
            return None;
        }
        self.advance();

        let TokenType::Identifier(var) = self.advance()? else {
            return None;
        };
        if !self.check(&TokenType::In) {
            return None;
        }
        self.advance();

        let arr = self.parse_until_brace()?;

        if !self.check(&TokenType::LeftBrace) {
            return None;
        }
        self.advance();

        let body = self.parse_until_close_brace()?;

        if !self.check(&TokenType::RightBrace) {
            return None;
        }
        self.advance();

        Some(Expr::FoldlExpr(
            acc_name,
            Box::new(init),
            var,
            Box::new(arr),
            Box::new(body),
        ))
    }

    fn parse_until_brace(&mut self) -> Option<Expr> {
        let start = self.pos;
        let mut depth = 0;
        while self.pos < self.tokens.len() {
            match self.peek()? {
                TokenType::LeftBrace if depth == 0 => break,
                TokenType::LeftParen | TokenType::LeftBracket => depth += 1,
                TokenType::RightParen | TokenType::RightBracket => depth -= 1,
                _ => {}
            }
            self.advance();
        }
        let end = self.pos;
        self.pos = start;

        let mut sub_parser = Parser::new(self.tokens[start..end].to_vec());
        let expr = sub_parser.parse_expr()?;
        self.pos = end;
        Some(expr)
    }

    fn parse_until_close_brace(&mut self) -> Option<Expr> {
        let start = self.pos;
        let mut depth = 0;
        while self.pos < self.tokens.len() {
            match self.peek()? {
                TokenType::RightBrace if depth == 0 => break,
                TokenType::LeftBrace => depth += 1,
                TokenType::RightBrace => depth -= 1,
                _ => {}
            }
            self.advance();
        }
        let end = self.pos;
        self.pos = start;

        let mut sub_parser = Parser::new(self.tokens[start..end].to_vec());
        let expr = sub_parser.parse_expr()?;
        self.pos = end;
        Some(expr)
    }

    fn parse_until_comma(&mut self) -> Option<Expr> {
        let start = self.pos;
        let mut depth = 0;
        while self.pos < self.tokens.len() {
            match self.peek()? {
                TokenType::Comma if depth == 0 => break,
                TokenType::LeftParen | TokenType::LeftBracket | TokenType::LeftBrace => depth += 1,
                TokenType::RightParen | TokenType::RightBracket | TokenType::RightBrace => {
                    depth -= 1
                }
                _ => {}
            }
            self.advance();
        }
        let end = self.pos;
        self.pos = start;

        let mut sub_parser = Parser::new(self.tokens[start..end].to_vec());
        let expr = sub_parser.parse_expr()?;
        self.pos = end;
        Some(expr)
    }
}
