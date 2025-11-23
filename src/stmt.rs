use ustr::{ustr, Ustr};

use crate::expr::Expr;
use crate::parser::Parser;
use crate::token::TokenType;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum Stmt {
    Print(Expr),
    Expression(Expr),
    Assign(Ustr, Expr),
    Block(Vec<Arc<Decl>>),
    Condition(Expr, Arc<Decl>, Option<Arc<Decl>>),
    While(Expr, Arc<Decl>),
    For(Ustr, Expr, Arc<Decl>),
}

#[derive(Debug, Clone, Default)]
pub struct Decl {
    pub v: DeclType,
    pub index: usize,
}

impl PartialEq for Decl {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for Decl {}

impl Hash for Decl {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

#[derive(Debug, Clone, Default)]
pub enum DeclType {
    VarDecl(Ustr, Expr, Mutability),
    Stmt(Arc<Stmt>),
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    Mutable,
    Immutable,
}

pub type TypeEnv = HashMap<String, (Mutability, Type)>;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Number,
    Bool,
    String,
    Nil,
    Array,
    Any,
}

pub struct StmtParser {
    tokens: Vec<TokenType>,
    pos: usize,
    type_env: TypeEnv,
    last_index: usize,
}

impl StmtParser {
    pub fn new(tokens: Vec<TokenType>) -> Self {
        StmtParser {
            tokens,
            pos: 0,
            type_env: HashMap::new(),
            last_index: 0,
        }
    }

    fn get_index(&mut self) -> usize {
        let index = self.last_index;
        self.last_index += 1;
        index
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

    pub fn parse(&mut self) -> Option<(Vec<Arc<Decl>>, TypeEnv)> {
        let mut decls = Vec::new();

        while self.pos < self.tokens.len() {
            decls.push(self.parse_decl()?);
        }

        Some((decls, self.type_env.clone()))
    }

    fn parse_decl(&mut self) -> Option<Arc<Decl>> {
        if self.check(&TokenType::Var) {
            self.parse_var_decl()
        } else if self.check(&TokenType::Final) {
            self.parse_final_decl()
        } else {
            self.parse_stmt_as_decl()
        }
    }

    fn parse_var_decl(&mut self) -> Option<Arc<Decl>> {
        self.advance(); // consume 'var'
        let TokenType::Identifier(name) = self.advance()? else {
            return None;
        };

        if !self.check(&TokenType::Equal) {
            return None;
        }
        self.advance();

        let expr = self.parse_expr_until_semicolon()?;

        if !self.check(&TokenType::Semicolon) {
            return None;
        }
        self.advance();

        self.type_env
            .insert(name.clone(), (Mutability::Mutable, Type::Any));

        Some(Arc::new(Decl {
            v: DeclType::VarDecl(ustr(&name), expr, Mutability::Mutable),
            index: self.get_index(),
        }))
    }

    fn parse_final_decl(&mut self) -> Option<Arc<Decl>> {
        self.advance(); // consume 'final'
        let TokenType::Identifier(name) = self.advance()? else {
            return None;
        };

        if !self.check(&TokenType::Equal) {
            return None;
        }
        self.advance();

        let expr = self.parse_expr_until_semicolon()?;

        if !self.check(&TokenType::Semicolon) {
            return None;
        }
        self.advance();

        self.type_env
            .insert(name.clone(), (Mutability::Immutable, Type::Any));

        Some(Arc::new(Decl {
            v: DeclType::VarDecl(ustr(&name), expr, Mutability::Immutable),
            index: self.get_index(),
        }))
    }

    fn parse_stmt_as_decl(&mut self) -> Option<Arc<Decl>> {
        let index = self.get_index();
        let stmt = self.parse_stmt()?;
        Some(Arc::new(Decl {
            v: DeclType::Stmt(Arc::new(stmt)),
            index,
        }))
    }

    fn parse_stmt(&mut self) -> Option<Stmt> {
        match self.peek()? {
            TokenType::Print => self.parse_print(),
            TokenType::LeftBrace => self.parse_block(),
            TokenType::If => self.parse_if(),
            TokenType::While => self.parse_while(),
            TokenType::For => self.parse_for(),
            TokenType::Identifier(_) => {
                // Check if it's an assignment
                let checkpoint = self.pos;
                self.advance(); // identifier
                if self.check(&TokenType::Equal) {
                    self.pos = checkpoint;
                    self.parse_assign()
                } else {
                    self.pos = checkpoint;
                    self.parse_expr_stmt()
                }
            }
            _ => self.parse_expr_stmt(),
        }
    }

    fn parse_print(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'print'
        let expr = self.parse_expr_until_semicolon()?;

        if !self.check(&TokenType::Semicolon) {
            return None;
        }
        self.advance();

        Some(Stmt::Print(expr))
    }

    fn parse_assign(&mut self) -> Option<Stmt> {
        let TokenType::Identifier(name) = self.advance()? else {
            return None;
        };

        // Check mutability
        if let Some((mutability, _)) = self.type_env.get(&name) {
            if *mutability == Mutability::Immutable {
                return None; // Cannot assign to immutable variable
            }
        } else {
            return None; // Variable not declared
        }

        if !self.check(&TokenType::Equal) {
            return None;
        }
        self.advance();

        let expr = self.parse_expr_until_semicolon()?;

        if !self.check(&TokenType::Semicolon) {
            return None;
        }
        self.advance();

        Some(Stmt::Assign(ustr(&name), expr))
    }

    fn parse_expr_stmt(&mut self) -> Option<Stmt> {
        let expr = self.parse_expr_until_semicolon()?;

        if !self.check(&TokenType::Semicolon) {
            return None;
        }
        self.advance();

        Some(Stmt::Expression(expr))
    }

    fn parse_block(&mut self) -> Option<Stmt> {
        self.advance(); // consume '{'

        let mut decls = Vec::new();

        while !self.check(&TokenType::RightBrace) && self.pos < self.tokens.len() {
            decls.push(self.parse_decl()?);
        }

        if !self.check(&TokenType::RightBrace) {
            return None;
        }
        self.advance();

        Some(Stmt::Block(decls))
    }

    fn parse_if(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'if'

        let cond = self.parse_expr_until_brace()?;

        let then_index = self.get_index();
        let then_stmt = self.parse_block()?;
        let then = Arc::new(Decl {
            v: DeclType::Stmt(Arc::new(then_stmt)),
            index: then_index,
        });

        let else_decl = if self.check(&TokenType::Else) {
            self.advance();

            let else_index = self.get_index();

            Some(Arc::new(Decl {
                v: DeclType::Stmt(Arc::new(self.parse_block()?)),
                index: else_index,
            }))
        } else {
            None
        };

        Some(Stmt::Condition(cond, then, else_decl))
    }

    fn parse_while(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'while'

        let cond = self.parse_expr_until_brace()?;

        let index = self.get_index();

        let body = self.parse_block()?;

        let body = Arc::new(Decl {
            v: DeclType::Stmt(Arc::new(body)),
            index,
        });

        Some(Stmt::While(cond, body))
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        self.advance(); // consume 'for'

        let TokenType::Identifier(var) = self.advance()? else {
            return None;
        };

        if !self.check(&TokenType::In) {
            return None;
        }
        self.advance();

        let arr = self.parse_expr_until_brace()?;

        let index = self.get_index();

        let body = self.parse_block()?;

        let body = Arc::new(Decl {
            v: DeclType::Stmt(Arc::new(body)),
            index,
        });

        Some(Stmt::For(ustr(&var), arr, body))
    }

    fn parse_expr_until_semicolon(&mut self) -> Option<Expr> {
        let start = self.pos;
        while self.pos < self.tokens.len() && !self.check(&TokenType::Semicolon) {
            self.advance();
        }
        let end = self.pos;

        let mut parser = Parser::new(self.tokens[start..end].to_vec());
        parser.parse_expr()
    }

    fn parse_expr_until_brace(&mut self) -> Option<Expr> {
        let start = self.pos;
        while self.pos < self.tokens.len() && !self.check(&TokenType::LeftBrace) {
            self.advance();
        }
        let end = self.pos;

        let mut parser = Parser::new(self.tokens[start..end].to_vec());
        parser.parse_expr()
    }
}
