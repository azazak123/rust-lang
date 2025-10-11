use std::collections::HashSet;

use crate::stmt::Decl;

#[derive(Clone, Debug, Default)]
pub struct DeclarationMeta {
    pub index: usize,
    pub complexity: usize,
    pub mut_deps: HashSet<usize>,
    pub class: Class,
    pub decl: Decl,
    pub loops_decls_indexes: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Class {
    #[default]
    Ordinary,
    Loop,
}
