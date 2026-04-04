#![forbid(unsafe_code)]

extern crate self as mel_ast;
extern crate self as mel_lexer;
extern crate self as mel_maya;
extern crate self as mel_parser;
extern crate self as mel_sema;
extern crate self as mel_syntax;

pub mod ast;
pub mod lexer;
pub mod maya;
pub mod parser;
pub mod sema;
pub mod syntax;

pub(crate) use maya::{model, normalize, registry, specialize, validate};
pub(crate) use parser::decode;
pub(crate) use sema::{command_norm, command_schema, scope};

pub use ast::*;
pub use lexer::*;
pub use maya::*;
pub use parser::*;
pub use sema::*;
pub use syntax::*;
