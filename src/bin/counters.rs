use aeron_rs::AeronClient;
use std::thread;
use std::time::Duration;

fn main() {
    println!("Connecting to Media Driver for Counters...");
    let mut client = AeronClient::new().expect("Failed to create AeronClient");
    client.start();

    // Give it a moment to connect and synchronize CNC metadata
    thread::sleep(Duration::from_millis(500));

    let reader = client.counters_reader();

    println!("--- AERON COUNTERS ---");
    let max_id = reader.max_counter_id();
    println!("Max Counter ID capacity: {}", max_id);

    reader.for_each(|id, type_id, _key_buffer, label| {
        let value = reader.get_counter_value(id);
        println!("{:>3} [{:<4}] {}: {}", id, type_id, label, value);
    });

    println!("----------------------");
}
