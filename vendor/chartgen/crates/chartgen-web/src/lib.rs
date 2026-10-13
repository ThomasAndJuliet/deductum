//! Browser binding. This module is distinct from Typst's byte-buffer ABI.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
pub fn dispatch(input: &str) -> String {
    String::from_utf8(chartgen_protocol::handle(input.as_bytes())).expect("JSON is UTF-8")
}
