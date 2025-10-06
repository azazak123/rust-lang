// src/token.rs
#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Minus,
    Plus,
    Semicolon,
    Slash,
    Star,
    Percent,
    Bang,
    BangEqual,
    Equal,
    EqualEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    DotDot,
    Identifier(String),
    StringLit(String),
    Number(f64),
    And,
    Or,
    If,
    Else,
    While,
    For,
    In,
    Print,
    Var,
    Final,
    True,
    False,
    Nil,
    Map,
    Filter,
    Scanl,
    Foldl,
}

pub fn tokenize(input: &str) -> Option<Vec<TokenType>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\n' | '\r' | '\t' => {
                chars.next();
            }
            '(' => {
                tokens.push(TokenType::LeftParen);
                chars.next();
            }
            ')' => {
                tokens.push(TokenType::RightParen);
                chars.next();
            }
            '{' => {
                tokens.push(TokenType::LeftBrace);
                chars.next();
            }
            '}' => {
                tokens.push(TokenType::RightBrace);
                chars.next();
            }
            '[' => {
                tokens.push(TokenType::LeftBracket);
                chars.next();
            }
            ']' => {
                tokens.push(TokenType::RightBracket);
                chars.next();
            }
            ',' => {
                tokens.push(TokenType::Comma);
                chars.next();
            }
            '+' => {
                tokens.push(TokenType::Plus);
                chars.next();
            }
            '-' => {
                tokens.push(TokenType::Minus);
                chars.next();
            }
            '*' => {
                tokens.push(TokenType::Star);
                chars.next();
            }
            '/' => {
                tokens.push(TokenType::Slash);
                chars.next();
            }
            ';' => {
                tokens.push(TokenType::Semicolon);
                chars.next();
            }
            '%' => {
                tokens.push(TokenType::Percent);
                chars.next();
            }

            '.' => {
                chars.next();
                if chars.peek() == Some(&'.') {
                    chars.next();
                    tokens.push(TokenType::DotDot);
                } else {
                    tokens.push(TokenType::Dot);
                }
            }

            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(TokenType::BangEqual);
                } else {
                    tokens.push(TokenType::Bang);
                }
            }

            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(TokenType::EqualEqual);
                } else {
                    tokens.push(TokenType::Equal);
                }
            }

            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(TokenType::LessEqual);
                } else {
                    tokens.push(TokenType::Less);
                }
            }

            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(TokenType::GreaterEqual);
                } else {
                    tokens.push(TokenType::Greater);
                }
            }

            '"' => {
                chars.next();
                let mut string = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        chars.next();
                        break;
                    }
                    string.push(c);
                    chars.next();
                }
                tokens.push(TokenType::StringLit(string));
            }

            '0'..='9' => {
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() {
                        num_str.push(c);
                        chars.next();
                    } else if c == '.' {
                        let mut peek = chars.clone();
                        peek.next();
                        if peek.peek() == Some(&'.') {
                            break;
                        }
                        num_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(TokenType::Number(num_str.parse().ok()?));
            }

            'a'..='z' | 'A'..='Z' | '_' => {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }

                let token = match ident.as_str() {
                    "and" => TokenType::And,
                    "or" => TokenType::Or,
                    "if" => TokenType::If,
                    "else" => TokenType::Else,
                    "while" => TokenType::While,
                    "for" => TokenType::For,
                    "in" => TokenType::In,
                    "print" => TokenType::Print,
                    "var" => TokenType::Var,
                    "final" => TokenType::Final,
                    "true" => TokenType::True,
                    "false" => TokenType::False,
                    "nil" => TokenType::Nil,
                    "map" => TokenType::Map,
                    "filter" => TokenType::Filter,
                    "scanl" => TokenType::Scanl,
                    "foldl" => TokenType::Foldl,
                    _ => TokenType::Identifier(ident),
                };
                tokens.push(token);
            }

            _ => return None,
        }
    }

    Some(tokens)
}
