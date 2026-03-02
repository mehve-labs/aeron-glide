use aeron_rs::MediaDriver;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Aeron Media Driver (Rust/CXX Wrapper)...");

    // Optional: Allow user to override the Aeron directory via environment variable
    let aeron_dir = env::var("AERON_DIR").unwrap_or_else(|_| "/dev/shm/aeron".to_string());
    println!("Aeron Directory: {}", aeron_dir);

    // Initialize the Media Driver
    let mut driver = MediaDriver::new();
    
    // Start the driver (this spawns the internal C driver threads)
    driver.start();
    println!("Media Driver started successfully.");
    println!("Press Ctrl+C to shut down...");

    // Set up a Ctrl+C handler to shut down cleanly
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    ctrlc::set_handler(move || {
        println!("\nShutting down Media Driver...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // Keep the main thread alive while the driver runs in the background
    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    println!("Media Driver stopped.");
    Ok(())
}
