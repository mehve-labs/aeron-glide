//! Multi-destination cast (MDC) publications and a multi-destination
//! subscription (MDS), over UDP on localhost with an embedded media driver:
//!
//! 1. Dynamic MDC (`control-mode=dynamic`): subscribers register with the
//!    publication's control endpoint and each receives the stream.
//! 2. Manual MDC (`control-mode=manual`): the publisher adds and removes
//!    destinations itself ([`add_destination`](aeron_glide::Publication::add_destination)).
//! 3. MDS (Aeron's `basic_mds_subscriber.c`): one subscription in manual control
//!    mode listens on both of those destinations, so it gets the stream over two
//!    transports, merged into one image (redundant feeds, each message once), and
//!    keeps receiving when one destination is removed.
//!
//! Self-contained:
//!
//! ```text
//! cargo run --example multi_destination
//! ```

use aeron_glide::{AeronClient, Context, MediaDriver, OfferError, Publication, Subscription};
use std::thread;
use std::time::{Duration, Instant};

const STREAM_ID: i32 = 1001;
const CONTROL: &str = "localhost:20151";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!("aeron-glide-mdc-{}", std::process::id()));
    let driver = MediaDriver::builder()
        .dir(&dir.to_string_lossy())
        .dir_delete_on_start(true)
        .dir_delete_on_shutdown(true)
        .start()?;
    let client = AeronClient::connect(Context::new().aeron_dir(driver.dir()))?;

    println!("--- Dynamic MDC: subscribers register with the control endpoint ---");
    let publication = client.add_publication(
        &format!("aeron:udp?control={CONTROL}|control-mode=dynamic"),
        STREAM_ID,
    )?;
    let mut subscribers = Vec::new();
    for port in [20152, 20153] {
        let channel = format!("aeron:udp?endpoint=localhost:{port}|control={CONTROL}");
        subscribers.push(client.add_subscription(&channel, STREAM_ID)?);
    }
    until("both subscribers to connect", || {
        subscribers.iter().all(Subscription::is_connected)
    });
    send(&publication, "dynamic", 3);
    for subscriber in &mut subscribers {
        println!(
            "{} received {:?}",
            subscriber.channel(),
            receive(subscriber, 3)?
        );
    }
    drop((publication, subscribers));

    println!("\n--- Manual MDC + MDS: one subscription on both destinations ---");
    let (a, b) = (
        "aeron:udp?endpoint=localhost:20154",
        "aeron:udp?endpoint=localhost:20155",
    );
    let mut mds = client.add_subscription("aeron:udp?control-mode=manual", STREAM_ID)?;
    for destination in [a, b] {
        let id = mds.add_destination(destination)?;
        until("the subscription destination", || {
            mds.find_destination_response(id).unwrap()
        });
    }
    let publication = client.add_publication("aeron:udp?control-mode=manual", STREAM_ID)?;
    let mut a_id = 0;
    for destination in [a, b] {
        let id = publication.add_destination(destination)?;
        until("the publication destination", || {
            publication.find_destination_response(id).unwrap()
        });
        if destination == a {
            a_id = id;
        }
    }
    until("both transports", || {
        mds.image_by_index(0)
            .is_some_and(|image| image.active_transport_count().unwrap() == 2)
    });
    send(&publication, "redundant", 3);
    println!("MDS received {:?}", receive(&mut mds, 3)?);
    let image = mds.image_by_index(0).expect("the image");
    println!(
        "{} image(s), session {}, {} active transports",
        mds.image_count(),
        image.session_id(),
        image.active_transport_count()?
    );

    // Losing one feed does not interrupt the stream.
    let removal = publication.remove_destination_by_id(a_id)?;
    until("the destination removal", || {
        publication.find_destination_response(removal).unwrap()
    });
    println!("Removed {a} from the publication");
    send(&publication, "after-removal", 3);
    println!("MDS received {:?}", receive(&mut mds, 3)?);
    Ok(())
}

/// Offer `count` numbered messages, retrying while not connected or back pressured.
fn send(publication: &Publication, prefix: &str, count: usize) {
    for i in 0..count {
        let message = format!("{prefix}-{i}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !sent(publication.offer(message.as_bytes())) {
            assert!(Instant::now() < deadline, "offer timed out");
            thread::yield_now();
        }
    }
}

/// Poll until `count` messages arrived; checks no more follow (no duplicates).
fn receive(subscription: &mut Subscription, count: usize) -> aeron_glide::Result<Vec<String>> {
    let mut messages = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while messages.len() < count {
        assert!(Instant::now() < deadline, "timed out receiving");
        subscription.poll(10, |data, _| {
            messages.push(String::from_utf8_lossy(data).into_owned())
        })?;
    }
    thread::sleep(Duration::from_millis(100));
    subscription.poll(10, |data, _| {
        panic!(
            "unexpected extra message {:?}",
            String::from_utf8_lossy(data)
        )
    })?;
    Ok(messages)
}

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(1));
    }
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
