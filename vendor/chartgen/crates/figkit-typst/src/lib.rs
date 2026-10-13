//! Typst plugin's byte-buffer ABI, without browser or WASI imports.
//!
//! Every input, including an oversized or malformed one, is answered with a
//! protocol response (an `error` outcome when it cannot be served), so the
//! Typst wrapper always receives a typed result instead of a bare plugin failure.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "typst_env")]
unsafe extern "C" {
    fn wasm_minimal_protocol_write_args_to_buffer(pointer: *mut u8);
    fn wasm_minimal_protocol_send_result_to_host(pointer: *const u8, length: usize);
}
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn dispatch(length: usize) -> i32 {
    let output = if length > figkit_protocol::MAX_INPUT_BYTES {
        // Do not allocate an oversized buffer; the protocol answers the size error itself.
        figkit_protocol::handle_oversized(length)
    } else {
        let mut input = vec![0u8; length];
        unsafe {
            wasm_minimal_protocol_write_args_to_buffer(input.as_mut_ptr());
        }
        figkit_protocol::handle(&input)
    };
    unsafe {
        wasm_minimal_protocol_send_result_to_host(output.as_ptr(), output.len());
    }
    0
}
