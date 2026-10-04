//! A running media driver's health from its CnC (command-and-control) file,
//! read without connecting a client: Aeron's `aeron_stat.c` / `AeronStat.java`
//! (the counters), `error_stat.c` / `ErrorStat.java` (the distinct error
//! log) and `loss_stat.c` / `LossStat.java` (streams that lost data) in one
//! tool.
//!
//! Needs a running media driver:
//!
//! ```text
//! cargo run --features bin --bin mediadriver
//! cargo run --example driver_stats
//! cargo run --example driver_stats -- --watch 1   # refresh every second
//! ```
//!
//! Options: `--dir` (the Aeron directory; `AERON_DIR` or the default one if
//! omitted) and `--watch <seconds>` (print the counters again until Ctrl-C).

use aeron_glide::{CncFile, Context, epoch_clock};
use clap::Parser;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Print a media driver's counters and error log (AeronStat + ErrorStat)")]
struct Args {
    /// The Aeron directory of the media driver (its default if omitted).
    #[arg(short = 'p', long)]
    dir: Option<String>,
    /// Print the counters again every this many seconds, until Ctrl-C.
    #[arg(short, long)]
    watch: Option<u64>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    // Like a client: `--dir`, else `AERON_DIR`, else the platform default.
    let dir = match args.dir.or_else(|| std::env::var("AERON_DIR").ok()) {
        Some(dir) => dir,
        None => Context::default_aeron_path()?,
    };

    // Waits up to a second for the file; a driver that exited may leave it behind.
    let cnc = CncFile::map_existing_with_timeout(&dir, Duration::from_secs(1))?;
    let constants = cnc.constants()?;
    let version = constants.cnc_version;
    let now = epoch_clock();
    println!("CnC file:       {}", cnc.file_name());
    println!(
        "CnC version:    {}.{}.{}",
        (version >> 16) & 0xff,
        (version >> 8) & 0xff,
        version & 0xff
    );
    println!(
        "Driver PID:     {} (started {:.1} s ago)",
        constants.pid,
        (now - constants.start_timestamp) as f64 / 1000.0
    );
    let heartbeat = match cnc.to_driver_heartbeat() {
        0 => "never".to_string(),
        timestamp => format!("{} ms ago", now - timestamp),
    };
    println!(
        "Heartbeat:      {heartbeat}, driver active: {}",
        cnc.is_driver_active(Duration::from_secs(10))
    );
    println!(
        "Buffers:        to-driver {} B, to-clients {} B, counters {} B, error log {} B",
        constants.to_driver_buffer_length,
        constants.to_clients_buffer_length,
        constants.counter_values_buffer_length,
        constants.error_log_buffer_length
    );
    println!(
        "Client liveness timeout: {:?}",
        constants.client_liveness_timeout
    );

    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    ctrlc::set_handler(move || flag.store(false, Ordering::Release))?;
    let reader = cnc.counters_reader();
    loop {
        // AeronStat's table: counter ID, value, type ID and label.
        println!("\n{:>4} {:>20} {:>5}  label", "id", "value", "type");
        let mut count = 0;
        reader.for_each(|id, type_id, _key, label| {
            let value = reader.get_counter_value(id).unwrap_or(0);
            println!("{id:>4} {value:>20} {type_id:>5}  {label}");
            count += 1;
        })?;
        println!("{count} counters");

        let Some(seconds) = args.watch else { break };
        std::thread::sleep(Duration::from_secs(seconds));
        if !running.load(Ordering::Acquire) {
            break;
        }
    }

    // ErrorStat: each distinct error once, with how often and when it occurred.
    println!("\nDistinct errors:");
    let now = epoch_clock();
    let errors = cnc.read_error_log(0, |entry| {
        println!(
            "***\n{} observations, first {:.1} s ago, last {:.1} s ago\n{}",
            entry.observation_count,
            (now - entry.first_observation_timestamp) as f64 / 1000.0,
            (now - entry.last_observation_timestamp) as f64 / 1000.0,
            entry.error
        );
    })?;
    println!("\n{errors} distinct errors observed.");

    // LossStat: each stream that lost data, with how much.
    println!("\nData loss:");
    let losses = cnc.read_loss_report(|entry| {
        println!(
            "{} observations, {} bytes lost, last {:.1} s ago: session {} stream {} {} from {}",
            entry.observation_count,
            entry.total_bytes_lost,
            (now - entry.last_observation_timestamp) as f64 / 1000.0,
            entry.session_id,
            entry.stream_id,
            entry.channel,
            entry.source
        );
    })?;
    println!("{losses} streams with loss.");
    Ok(())
}
