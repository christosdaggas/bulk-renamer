//! Expression engine module.
//!
//! This module provides a Rust-based expression DSL for advanced renaming operations.

mod parser;
mod evaluator;

pub use evaluator::ExpressionEngine;
pub use parser::*;
