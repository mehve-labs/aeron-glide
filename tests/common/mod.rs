//! Shared fixtures for integration tests.
//!
//! Every test starts its own embedded media driver in a unique directory, so
//! tests are independent and run in parallel.

#![allow(dead_code)]

use aeron_glide::{AeronClient, Context, MediaDriver, Subscription, ThreadingMode};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Default deadline for anything a test waits for.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// An embedded media driver in its own directory, removed on drop.
pub struct TestDriver {
    pub driver: MediaDriver,
    pub dir: String,
}

impl TestDriver {
    pub fn start() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aeron-glide-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let dir = dir.to_str().expect("utf-8 temp dir").to_string();
        let driver = MediaDriver::builder()
            .dir(&dir)
            .dir_delete_on_start(true)
            .dir_delete_on_shutdown(true)
            .threading_mode(ThreadingMode::Shared)
            // Small IPC terms keep memory low; max message length is term / 8 = 128 KiB.
            .ipc_term_buffer_length(1 << 20)
            // Close publications (and their images) quickly once released.
            .publication_linger_timeout_ns(Duration::from_millis(50).as_nanos() as u64)
            .start()
            .expect("start media driver");
        Self { driver, dir }
    }

    /// A client connected to this driver.
    pub fn client(&self) -> AeronClient {
        self.connect(Context::new())
    }

    /// A client connected to this driver with extra settings.
    pub fn connect(&self, context: Context) -> AeronClient {
        AeronClient::connect(context.aeron_dir(&self.dir)).expect("connect client")
    }
}

impl Drop for TestDriver {
    fn drop(&mut self) {
        // The driver deletes its directory on shutdown; this covers a failed start.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Wait until `condition` holds, panicking with `what` after [`TIMEOUT`].
pub fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Wait until the subscription has a connected publisher.
pub fn wait_connected(sub: &Subscription) {
    wait_until("subscription to connect", || sub.is_connected());
}

/// Offer until it succeeds, failing on a non-retryable error or after [`TIMEOUT`].
pub fn offer(publication: &aeron_glide::Publication, message: &[u8]) -> i64 {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match publication.offer(message) {
            Ok(position) => return position,
            Err(e) if e.is_retryable() && Instant::now() < deadline => std::thread::yield_now(),
            Err(e) => panic!("offer failed: {e}"),
        }
    }
}

/// Poll until `handler` has seen `count` fragments in total.
pub fn poll_n(sub: &mut Subscription, count: usize, mut handler: impl FnMut(&[u8])) {
    let mut seen = 0;
    wait_until("fragments", || {
        sub.poll(10, |data| {
            seen += 1;
            handler(data);
        })
        .expect("poll");
        seen >= count
    });
}
