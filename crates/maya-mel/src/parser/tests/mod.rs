use super::{
    LightItem, LightParseOptions, LightWord, ParseBudgets, ParseMode, ParseOptions, SourceEncoding,
    parse_bytes, parse_bytes_with_encoding, parse_file, parse_file_with_encoding,
    parse_light_bytes, parse_light_bytes_with_encoding, parse_light_file, parse_light_shared_bytes,
    parse_light_shared_bytes_with_encoding, parse_light_shared_file, parse_light_shared_source,
    parse_light_source, parse_light_source_with_options, parse_shared_bytes,
    parse_shared_bytes_with_encoding, parse_shared_file, parse_shared_file_with_encoding,
    parse_shared_source, parse_source, parse_source_view_range, parse_source_with_options,
    scan_light_bytes_with_sink, scan_light_file_with_encoding_and_options_and_sink,
    scan_light_shared_bytes_with_encoding_and_options_and_sink,
    scan_light_shared_bytes_with_options_and_sink,
    scan_light_shared_file_with_encoding_and_options_and_sink,
    scan_light_shared_source_with_options_and_sink, scan_light_source_with_options_and_sink,
};
use encoding_rs::{GBK, SHIFT_JIS};
use mel_ast::{
    AssignOp, BinaryOp, Expr, InvokeSurface, Item, ShellWord, Stmt, SwitchLabel, TypeName, UnaryOp,
    UpdateOp, VectorComponent,
};
use mel_syntax::text_range;
use std::{
    fs,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

mod commands;
mod decode;
mod diagnostics;
mod expressions;
mod light;
mod proc;
