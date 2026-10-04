//! The Rust snippets of README.md, compiled (not run: they need a media driver
//! or an archive), and a test that the README still shows exactly these.
//!
//! When the API changes, these stop compiling; when the README changes, update
//! the copy here too.
#![allow(dead_code, unused_variables, clippy::all)]

#[rustfmt::skip]
fn quick_start() -> aeron_glide::Result<()> {
// README: quick start
use aeron_glide::AeronClient;

let client = AeronClient::new()?; // connects to a running media driver
let publication = client.add_publication("aeron:ipc", 1001)?;
let mut subscription = client.add_subscription("aeron:ipc", 1001)?;

// Retry while not connected or back pressured; other errors are real.
while let Err(e) = publication.offer(b"hello aeron") {
    if !e.is_retryable() {
        return Err(e.into());
    }
}

subscription.poll(10, |data, _header| {
    println!("received {}", String::from_utf8_lossy(data));
})?;
// end
Ok(())
}

#[rustfmt::skip]
fn compared_with_rusteron(dir: &str) -> aeron_glide::Result<()> {
use aeron_glide::{AeronClient, Context};
// README: aeron-glide
let client = AeronClient::connect(Context::new().aeron_dir(dir))?;
let publication = client.add_publication("aeron:ipc", 123)?;
let mut subscription = client.add_subscription("aeron:ipc", 123)?;
subscription.poll(10, |msg, header| { /* ... */ })?;
// end
Ok(())
}

#[cfg(feature = "archive")]
#[rustfmt::skip]
fn archive() -> Result<(), Box<dyn std::error::Error>> {
// README: archive
use aeron_glide::archive::{self, ReplayParams, SourceLocation};

let archive = archive::Context::new()
    .control_request_channel("aeron:udp?endpoint=localhost:8010")
    .control_response_channel("aeron:udp?endpoint=localhost:0")
    .connect()?;

let subscription_id =
    archive.start_recording("aeron:ipc", 1001, SourceLocation::Local, false)?;
archive.list_recordings(0, 100, |recording| {
    println!("{}: {}", recording.recording_id, recording.stripped_channel);
})?;
let mut replay = archive.replay(0, "aeron:ipc", 1002, &ReplayParams::new().position(0))?;
replay.poll(10, |data, _| println!("{} bytes", data.len()))?;
// end
Ok(())
}

/// Code with all whitespace removed, to compare snippets regardless of layout.
fn squash(code: &str) -> String {
    code.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The Rust code blocks of `markdown`.
fn rust_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match (&mut current, line.trim_start()) {
            (None, l) if l.starts_with("```rust") => current = Some(String::new()),
            (Some(_), l) if l.starts_with("```") => blocks.push(current.take().unwrap()),
            (Some(block), _) => {
                block.push_str(line);
                block.push('\n');
            }
            _ => {}
        }
    }
    blocks
}

#[test]
fn readme_snippets_are_the_compiled_ones() {
    let readme = include_str!("../README.md");
    let compiled = squash(include_str!("readme.rs"));
    let blocks = rust_blocks(readme);
    assert_eq!(
        blocks.len(),
        3,
        "a README snippet was added or removed: update tests/readme.rs"
    );
    for block in blocks {
        // The comparison with rusteron: only aeron-glide's half compiles here.
        let block = match block.split_once("// aeron-glide") {
            Some((_, ours)) => ours.to_string(),
            None => block,
        };
        assert!(
            compiled.contains(&squash(&block)),
            "this README snippet differs from its copy in tests/readme.rs:\n{block}"
        );
    }
}
