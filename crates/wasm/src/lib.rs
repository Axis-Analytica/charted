//! The DAX editor's language service for the browser: diagnostics, completion,
//! signature help, hover and formatting over the current model. Offsets in and out are
//! UTF-16 code units, as JavaScript strings count them; results are JSON strings.

use charted_dax::{Span, ide};
use charted_model::Model;
use serde::Serialize;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::default());
}

/// UTF-16 offset → byte offset in `text`.
fn to_byte(text: &str, utf16: u32) -> usize {
    let mut units = 0u32;
    for (i, c) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

/// Byte offset → UTF-16 offset in `text`.
fn to_utf16(text: &str, byte: usize) -> u32 {
    text[..byte.min(text.len())].encode_utf16().count() as u32
}

#[derive(Serialize)]
struct Range {
    start: u32,
    end: u32,
}

fn range(text: &str, s: Span) -> Range {
    Range {
        start: to_utf16(text, s.start),
        end: to_utf16(text, s.end),
    }
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// Replaces the model the service resolves names against (JSON `Model`).
#[wasm_bindgen(js_name = setModel)]
pub fn set_model(model_json: &str) -> Result<(), JsError> {
    let m: Model = serde_json::from_str(model_json)?;
    MODEL.with(|cell| *cell.borrow_mut() = m);
    Ok(())
}

#[derive(Serialize)]
struct Diag {
    message: String,
    start: u32,
    end: u32,
}

/// Problems in a formula: parse errors and unknown or misused names. `home` is a
/// calculated column's table (row context); omit it for measures.
#[wasm_bindgen]
pub fn diagnostics(text: &str, home: Option<String>) -> String {
    MODEL.with(|cell| {
        let model = cell.borrow();
        let (_, diags) = charted_dax::check_in(&model, text, home.as_deref());
        let out: Vec<Diag> = diags
            .into_iter()
            .map(|d| Diag {
                start: to_utf16(text, d.span.start),
                end: to_utf16(text, d.span.end.max(d.span.start)),
                message: d.message,
            })
            .collect();
        json(&out)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompletionOut {
    label: String,
    kind: ide::CompletionKind,
    detail: String,
    documentation: Option<String>,
    insert_text: String,
    replace: Range,
    sort: String,
}

#[wasm_bindgen]
pub fn complete(text: &str, offset: u32, home: Option<String>) -> String {
    MODEL.with(|cell| {
        let model = cell.borrow();
        let items = ide::complete(&model, text, to_byte(text, offset), home.as_deref());
        let out: Vec<CompletionOut> = items
            .into_iter()
            .map(|c| CompletionOut {
                replace: range(text, c.replace),
                label: c.label,
                kind: c.kind,
                detail: c.detail,
                documentation: c.documentation,
                insert_text: c.insert_text,
                sort: c.sort,
            })
            .collect();
        json(&out)
    })
}

#[wasm_bindgen]
pub fn signature(text: &str, offset: u32) -> String {
    json(&ide::signature(text, to_byte(text, offset)))
}

#[derive(Serialize)]
struct HoverOut {
    contents: String,
    range: Range,
}

#[wasm_bindgen]
pub fn hover(text: &str, offset: u32, home: Option<String>) -> String {
    MODEL.with(|cell| {
        let model = cell.borrow();
        json(
            &ide::hover(&model, text, to_byte(text, offset), home.as_deref()).map(|h| HoverOut {
                range: range(text, h.span),
                contents: h.contents,
            }),
        )
    })
}

#[wasm_bindgen]
pub fn format(text: &str) -> String {
    ide::format(text)
}

/// Every known function, for listings (JSON `FunctionDoc[]`).
#[wasm_bindgen]
pub fn functions() -> String {
    json(&charted_dax::catalog::functions())
}

#[cfg(test)]
mod tests {
    #[test]
    fn offsets_round_trip_through_utf16() {
        let s = "'Café 😀'[x]";
        for (i, _) in s.char_indices() {
            assert_eq!(super::to_byte(s, super::to_utf16(s, i)), i);
        }
    }
}
