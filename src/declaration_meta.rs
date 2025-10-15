use std::{collections::HashSet, sync::Arc};

use crate::stmt::{Decl, DeclType};

#[derive(Clone, Debug, Default)]
pub struct DeclarationMeta {
    // pub index: usize,
    pub complexity: usize,
    pub mut_deps: HashSet<usize>,
    pub class: Class,
    pub decl: Arc<Decl>,
    pub loops_decls_indexes: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Class {
    #[default]
    Ordinary,
    Loop,
}
