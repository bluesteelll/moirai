//! LQ-3, the reference model's front end for the query language ([50 §8.2] LQ-3, PLAN WP-93a): its own lexer and
//! parser for grammar v1 ([LQ/grammar-v1.ebnf], [LQ/lexical]) including `TX` blocks and the strict-GQL spelling mode
//! ([LQ/gql-spelling §3]), the S-AST and its S-expression form, the display printer with the property
//! `parse(print(ast)) == ast`, and the binder that turns an S-AST into the canonical AST and its encoding
//! ([LQ/canonical-ast]) with the diagnostics of [LQ/errors] and the reading echo of [LQ/envelope §4].
//!
//! Written from the specification only (S2): no engine code is read or shared.

pub mod ast;
pub mod bind;
pub mod cast;
pub mod catalog;
pub mod ctx;
pub mod diag;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod schema;
pub mod sexpr;

#[cfg(test)]
mod tests;
