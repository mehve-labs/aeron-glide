#![cfg(feature = "driver")]
mod common;

use common::{TestDriver, offer, wait_connected, wait_until};
use std::sync::Arc;

#[test]
fn shared_client_and_publication_across_threads() {
    let driver = TestDriver::start();
    let client = Arc::new(driver.client());
    let publication = Arc::new(client.add_publication("aeron:ipc", 1).unwrap());
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);

    let workers: Vec<_> = (0..4u8)
        .map(|i| {
            let (client, publication) = (client.clone(), publication.clone());
            std::thread::spawn(move || {
                client.add_publication("aeron:ipc", 100 + i as i32).unwrap();
                for _ in 0..100 {
                    offer(&publication, &[i]);
                }
            })
        })
        .collect();
    let mut got = [0u32; 4];
    wait_until("400 messages", || {
        sub.poll(100, |data, _| got[data[0] as usize] += 1).unwrap();
        got.iter().sum::<u32>() == 400
    });
    workers.into_iter().for_each(|w| w.join().unwrap());
    assert_eq!(got, [100; 4]);
}

#[test]
fn subscription_moves_to_another_thread() {
    let driver = TestDriver::start();
    let client = driver.client();
    let publication = client.add_publication("aeron:ipc", 1).unwrap();
    let mut sub = client.add_subscription("aeron:ipc", 1).unwrap();
    wait_connected(&sub);
    offer(&publication, b"moved");

    let received = std::thread::spawn(move || {
        let mut received = Vec::new();
        common::poll_n(&mut sub, 1, |data| received.push(data.to_vec()));
        received
    })
    .join()
    .unwrap();
    assert_eq!(received, [b"moved".to_vec()]);
}
