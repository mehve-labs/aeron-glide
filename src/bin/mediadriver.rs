use aeron_glide::{DriverIdleStrategy, MediaDriver, ThreadingMode};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    dir: Option<String>,
    dir_delete_on_start: Option<bool>,
    dir_delete_on_shutdown: Option<bool>,
    threading_mode: Option<String>,
    conductor_idle_strategy: Option<String>,
    sender_idle_strategy: Option<String>,
    receiver_idle_strategy: Option<String>,
    term_buffer_length: Option<usize>,
    ipc_term_buffer_length: Option<usize>,
    mtu_length: Option<usize>,
    ipc_mtu_length: Option<usize>,
    socket_so_rcvbuf: Option<usize>,
    socket_so_sndbuf: Option<usize>,
    print_configuration: Option<bool>,
    conductor_cpu_affinity: Option<i32>,
    sender_cpu_affinity: Option<i32>,
    receiver_cpu_affinity: Option<i32>,
    /// Accept termination requests carrying this token (e.g. from
    /// `Context::request_driver_termination`) and shut down.
    termination_token: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::args().nth(1);

    let config = if let Some(ref path) = config_path {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path, e))?;
        let cfg: Config = serde_norway::from_str(&contents)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path, e))?;
        println!("Loaded config from: {}", path);
        cfg
    } else {
        println!("No config file specified, using Aeron defaults.");
        Config::default()
    };

    println!("Starting Aeron Media Driver...");

    let mut builder = MediaDriver::builder();

    // Apply configuration
    if let Some(ref dir) = config.dir {
        builder = builder.dir(dir);
    }
    if let Some(v) = config.dir_delete_on_start {
        builder = builder.dir_delete_on_start(v);
    }
    if let Some(v) = config.dir_delete_on_shutdown {
        builder = builder.dir_delete_on_shutdown(v);
    }
    if let Some(ref mode) = config.threading_mode {
        builder = builder.threading_mode(mode.parse::<ThreadingMode>()?);
    }
    if let Some(ref s) = config.conductor_idle_strategy {
        builder = builder.conductor_idle_strategy(s.parse::<DriverIdleStrategy>()?);
    }
    if let Some(ref s) = config.sender_idle_strategy {
        builder = builder.sender_idle_strategy(s.parse::<DriverIdleStrategy>()?);
    }
    if let Some(ref s) = config.receiver_idle_strategy {
        builder = builder.receiver_idle_strategy(s.parse::<DriverIdleStrategy>()?);
    }
    if let Some(v) = config.term_buffer_length {
        builder = builder.term_buffer_length(v);
    }
    if let Some(v) = config.ipc_term_buffer_length {
        builder = builder.ipc_term_buffer_length(v);
    }
    if let Some(v) = config.mtu_length {
        builder = builder.mtu_length(v);
    }
    if let Some(v) = config.ipc_mtu_length {
        builder = builder.ipc_mtu_length(v);
    }
    if let Some(v) = config.socket_so_rcvbuf {
        builder = builder.socket_so_rcvbuf(v);
    }
    if let Some(v) = config.socket_so_sndbuf {
        builder = builder.socket_so_sndbuf(v);
    }
    if let Some(v) = config.print_configuration {
        builder = builder.print_configuration(v);
    }
    if let Some(v) = config.conductor_cpu_affinity {
        builder = builder.conductor_cpu_affinity(v);
    }
    if let Some(v) = config.sender_cpu_affinity {
        builder = builder.sender_cpu_affinity(v);
    }
    if let Some(v) = config.receiver_cpu_affinity {
        builder = builder.receiver_cpu_affinity(v);
    }

    let running = Arc::new(AtomicBool::new(true));
    // Stop on SIGINT, SIGTERM or SIGHUP (e.g. systemd, Kubernetes), from before
    // the driver starts, so the driver is always closed (and its directory
    // deleted if configured).
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nShutting down Media Driver...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting the signal handler");

    // Stop on an accepted termination request: one carrying the configured
    // token, or whatever AERON_DRIVER_TERMINATION_VALIDATOR accepts.
    if let Some(token) = config.termination_token.clone() {
        builder = builder.termination_validator(move |request| request == token.as_bytes());
    }
    let r = running.clone();
    builder = builder.termination_hook(move || {
        println!("\nTermination requested, shutting down Media Driver...");
        r.store(false, Ordering::SeqCst);
    });

    let driver = builder.start()?;
    let invoker = driver.threading_mode() == ThreadingMode::Invoker;
    println!("Media Driver started in {}", driver.dir());
    println!("Media Driver started successfully.");
    println!("Press Ctrl+C to shut down...");

    while running.load(Ordering::SeqCst) {
        if invoker {
            // No driver threads: run its duty cycle here.
            let work = driver.do_work()?;
            driver.idle(work)?;
        } else {
            thread::sleep(Duration::from_millis(100));
        }
    }

    println!("Media Driver stopped.");
    Ok(())
}
