//! Expression parser using Pest grammar.

use pest_derive::Parser;

/// The expression parser.
#[derive(Parser)]
#[grammar = "expression/grammar.pest"]
pub struct ExpressionParser;
