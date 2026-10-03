//! Send a file over Aeron and reassemble it, like Aeron's `FileSender.java` /
//! `FileReceiver.java`: a "file create" message announces the correlation ID,
//! length and name, then the content follows in chunks, each stamped with its
//! offset and written with zero-copy `try_claim` (so a chunk is at most one
//! frame's payload). The receiver rebuilds the file and the example checks the
//! copy against the original (length and FNV-1a checksum).
//!
//! Self-contained (an embedded media driver, sender and receiver in one
//! process). Sends `--file`, or a generated file of `--size` bytes:
//!
//! ```text
//! cargo run --example file_transfer
//! cargo run --example file_transfer -- --file Cargo.lock
//! ```

use aeron_glide::{AeronClient, Context, MediaDriver, OfferError, ThreadingMode};
use clap::Parser;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const CHANNEL: &str = "aeron:udp?endpoint=localhost:20141";
const STREAM_ID: i32 = 1001;

// Message layout (little endian), as in FileSender: version, type, correlation ID, then
// the file length and name (FILE_CREATE) or the chunk offset, length and bytes (FILE_CHUNK).
const VERSION: u32 = 0;
const FILE_CREATE: u32 = 1;
const FILE_CHUNK: u32 = 2;
const CHUNK_PAYLOAD_OFFSET: usize = 32;

#[derive(Parser)]
#[command(about = "Send a file in chunks and reassemble it (FileSender / FileReceiver)")]
struct Args {
    /// The file to send (a generated one if omitted).
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// The size of the generated file, in bytes.
    #[arg(short, long, default_value_t = 1024 * 1024)]
    size: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (name, content) = match &args.file {
        Some(path) => (path.display().to_string(), std::fs::read(path)?),
        None => ("generated.bin".to_string(), generate(args.size)),
    };

    let dir = std::env::temp_dir().join(format!("aeron-glide-file-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .threading_mode(ThreadingMode::Shared)
        .start()?;
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;
    let mut subscription = client.add_subscription(CHANNEL, STREAM_ID)?;
    let mut publication = client.add_exclusive_publication(CHANNEL, STREAM_ID)?;
    let start = Instant::now();

    let received = thread::scope(|scope| {
        // Sender: announce the file, then stream it in claimed chunks.
        scope.spawn(|| {
            let correlation_id = client.next_correlation_id();
            let mut create = header(FILE_CREATE, correlation_id);
            create.extend_from_slice(&(content.len() as u64).to_le_bytes());
            create.extend_from_slice(name.as_bytes());
            while !sent(publication.offer(&create)) {
                thread::yield_now();
            }
            let max_chunk = publication.max_payload_length() - CHUNK_PAYLOAD_OFFSET;
            for (i, chunk) in content.chunks(max_chunk).enumerate() {
                let length = CHUNK_PAYLOAD_OFFSET + chunk.len();
                let mut claim = loop {
                    match publication.try_claim(length) {
                        Ok(claim) => break claim,
                        Err(e) if e.is_retryable() => thread::yield_now(),
                        Err(e) => panic!("try_claim failed: {e}"),
                    }
                };
                let buffer = claim.buffer_mut();
                buffer[..16].copy_from_slice(&header(FILE_CHUNK, correlation_id));
                buffer[16..24].copy_from_slice(&((i * max_chunk) as u64).to_le_bytes());
                buffer[24..32].copy_from_slice(&(chunk.len() as u64).to_le_bytes());
                buffer[CHUNK_PAYLOAD_OFFSET..].copy_from_slice(chunk);
                claim.commit();
            }
            println!(
                "Sent {name}: {} bytes in {max_chunk}-byte chunks",
                content.len()
            );
        });

        // Receiver: rebuild the file until every byte has arrived.
        let mut file: Option<(String, Vec<u8>)> = None;
        let mut remaining = usize::MAX;
        let deadline = Instant::now() + Duration::from_secs(30);
        while remaining > 0 {
            assert!(Instant::now() < deadline, "timed out receiving the file");
            let fragments = subscription.poll(10, |data, _| {
                let u32_at = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
                let u64_at = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
                assert_eq!(u32_at(0), VERSION, "unknown version");
                match u32_at(4) {
                    FILE_CREATE => {
                        let length = u64_at(16) as usize;
                        let name = String::from_utf8_lossy(&data[24..]).into_owned();
                        println!(
                            "Receiving {name} ({length} bytes, correlation ID {})",
                            u64_at(8)
                        );
                        file = Some((name, vec![0; length]));
                        remaining = length;
                    }
                    FILE_CHUNK => {
                        let (_, bytes) = file.as_mut().expect("a chunk before the file create");
                        let (offset, length) = (u64_at(16) as usize, u64_at(24) as usize);
                        bytes[offset..offset + length]
                            .copy_from_slice(&data[CHUNK_PAYLOAD_OFFSET..][..length]);
                        remaining -= length;
                    }
                    other => panic!("unknown message type {other}"),
                }
            })?;
            if fragments == 0 {
                thread::yield_now();
            }
        }
        Ok::<_, aeron_glide::Error>(file.expect("the file"))
    })?;

    let (received_name, copy) = received;
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "Received {received_name}: {} bytes in {:.3} s ({:.1} MB/s)",
        copy.len(),
        elapsed,
        copy.len() as f64 / elapsed / 1e6
    );
    println!(
        "checksums: sent {:016x}, received {:016x}",
        fnv1a(&content),
        fnv1a(&copy)
    );
    assert!(
        copy == content,
        "the received file differs from the one sent"
    );
    println!("The received copy matches the original.");
    Ok(())
}

/// The version, message type and correlation ID that start every message.
fn header(message_type: u32, correlation_id: i64) -> Vec<u8> {
    let mut header = Vec::with_capacity(64);
    header.extend_from_slice(&VERSION.to_le_bytes());
    header.extend_from_slice(&message_type.to_le_bytes());
    header.extend_from_slice(&correlation_id.to_le_bytes());
    header
}

/// Pseudo-random content (xorshift), so a corrupted chunk would not go unnoticed.
fn generate(size: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// 64-bit FNV-1a checksum.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// `true` once offered, `false` to retry (back pressure, not connected yet, ...).
/// Errors that retrying cannot fix (e.g. a message too long) end the example.
fn sent(result: Result<i64, OfferError>) -> bool {
    match result {
        Ok(_) => true,
        Err(e) if e.is_retryable() => false,
        Err(e) => panic!("offer failed: {e}"),
    }
}
