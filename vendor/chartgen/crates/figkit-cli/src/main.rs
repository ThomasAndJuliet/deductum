//! File/process ownership belongs to this host executable, never to the core.
use std::io::{Read, Write};
fn main() -> std::io::Result<()> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(figkit_protocol::MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut input)?;
    std::io::stdout().write_all(&figkit_protocol::handle(&input))
}
