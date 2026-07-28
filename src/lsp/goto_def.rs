// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{GotoDefinitionResponse, Location, Position, Url};

use crate::semantic::SemanticReport;

use super::hover::word_at_offset;
use super::span::{position_to_byte_offset, span_to_range};

pub fn goto_definition(
    report: &SemanticReport,
    source: &str,
    uri: &Url,
    pos: Position,
) -> Option<GotoDefinitionResponse> {
    let offset = position_to_byte_offset(pos, source)?;
    let word = word_at_offset(source, offset)?;

    let entry = report
        .symbol_table
        .entries
        .iter()
        .find(|e| e.name == word && e.symbol.span.start != 0)?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: span_to_range(entry.symbol.span, source),
    }))
}
