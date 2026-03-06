use aeron_glide::{AeronClient, ControlledAction};
use std::thread;
use std::time::Duration;

const CHANNEL: &str = "aeron:ipc";

fn pick_stream_id() -> i32 {
    // Random stream ID to avoid collisions with stale publications from previous runs.
    // The media driver keeps shared-memory log buffers alive briefly after a client exits.
    (std::process::id() as i32).wrapping_mul(7) | 1
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stream_id = pick_stream_id();
    let mut client = AeronClient::new()?;
    client.start();

    // Exclusive publications each get their own session — this is what creates
    // separate Images on the subscriber side. Regular publications on the same
    // channel+stream share a session (even across clients).
    let mut pub1 = client.add_exclusive_publication(CHANNEL, stream_id)?;
    let mut pub2 = client.add_exclusive_publication(CHANNEL, stream_id)?;
    let mut sub = client.add_subscription(CHANNEL, stream_id)?;

    // Wait for both images to appear
    println!("Waiting for publishers to connect...");
    while sub.image_count() < 2 {
        thread::sleep(Duration::from_millis(10));
    }
    println!("Connected: {} images\n", sub.image_count());

    // Publish some messages from each publisher
    for i in 0..5 {
        let msg1 = format!("pub1: message #{}", i);
        while pub1.offer(msg1.as_bytes()) < 0 {
            thread::yield_now();
        }

        let msg2 = format!("pub2: message #{}", i);
        while pub2.offer(msg2.as_bytes()) < 0 {
            thread::yield_now();
        }
    }
    thread::sleep(Duration::from_millis(100));

    // --- Image API demo ---

    let count = sub.image_count();
    println!("=== {} Active Images (one per publisher) ===\n", count);

    for i in 0..count as usize {
        let image = sub.image_by_index(i)?;
        println!(
            "  Image[{}]: session_id={:<10} correlation_id={} join_position={} source=\"{}\"",
            i,
            image.session_id(),
            image.correlation_id(),
            image.join_position(),
            image.source_identity(),
        );
        println!(
            "            position={} closed={} end_of_stream={}",
            image.position(),
            image.is_closed(),
            image.is_end_of_stream(),
        );
    }

    // Poll messages per-image using raw poll — each image only sees its own publisher
    println!("\n=== Per-Image Raw Poll ===");
    println!("  (Each image only contains messages from its publisher)\n");
    for i in 0..count as usize {
        let mut image = sub.image_by_index(i)?;
        let sid = image.session_id();
        let fragments = image.poll(10, |data| {
            let msg = std::str::from_utf8(data).unwrap_or("<binary>");
            println!("  [session={}] {}", sid, msg);
        });
        println!(
            "  -> {} fragments, position now={}\n",
            fragments,
            image.position()
        );
    }

    // Publish more messages for assembled poll demo
    for i in 5..10 {
        let msg1 = format!("pub1: message #{}", i);
        while pub1.offer(msg1.as_bytes()) < 0 {
            thread::yield_now();
        }
        let msg2 = format!("pub2: message #{}", i);
        while pub2.offer(msg2.as_bytes()) < 0 {
            thread::yield_now();
        }
    }
    thread::sleep(Duration::from_millis(100));

    // Assembled poll with auto-Continue
    println!("=== Per-Image Assembled Poll ===\n");
    for i in 0..count as usize {
        let mut image = sub.image_by_index(i)?;
        let sid = image.session_id();
        let fragments = image.poll_assembled(10, |data| {
            let msg = std::str::from_utf8(data).unwrap_or("<binary>");
            println!("  [session={}] {}", sid, msg);
        });
        println!("  -> {} fragments (auto-Continue)\n", fragments);
    }

    // Demonstrate ControlledAction::Break — stop after 2 messages
    println!("=== Flow Control: Break after 2 messages ===\n");
    for j in 10..15 {
        let msg1 = format!("pub1: extra #{}", j);
        while pub1.offer(msg1.as_bytes()) < 0 {
            thread::yield_now();
        }
        let msg2 = format!("pub2: extra #{}", j);
        while pub2.offer(msg2.as_bytes()) < 0 {
            thread::yield_now();
        }
    }
    thread::sleep(Duration::from_millis(100));

    for i in 0..count as usize {
        let mut image = sub.image_by_index(i)?;
        let sid = image.session_id();
        let pos_before = image.position();

        let mut seen = 0;
        let fragments = image.poll_assembled(10, |data| -> ControlledAction {
            let msg = std::str::from_utf8(data).unwrap_or("<binary>");
            seen += 1;
            println!("  [session={}] {} (seen={})", sid, msg, seen);
            if seen >= 2 {
                ControlledAction::Break
            } else {
                ControlledAction::Continue
            }
        });
        println!(
            "  -> {} fragments delivered, position {} -> {} (remaining still queued)\n",
            fragments,
            pos_before,
            image.position()
        );
    }

    // Lookup by session_id
    println!("=== Lookup by Session ID ===\n");
    let img0 = sub.image_by_index(0)?;
    let sid = img0.session_id();
    let img_lookup = sub.image_by_session_id(sid)?;
    println!(
        "  image_by_session_id({}) -> position={}",
        sid,
        img_lookup.position()
    );

    // Final positions
    println!("\n=== Final Position Tracking ===\n");
    for i in 0..count as usize {
        let image = sub.image_by_index(i)?;
        println!(
            "  Image[{}] session={}: position={}",
            i,
            image.session_id(),
            image.position()
        );
    }

    println!("\nDone!");
    Ok(())
}
