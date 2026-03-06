use aeron_glide::{IdleStrategy, MediaDriver, ThreadingMode};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Default, Deserialize)]
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
}

fn parse_threading_mode(s: &str) -> Result<ThreadingMode, String> {
    match s {
        "dedicated" => Ok(ThreadingMode::Dedicated),
        "shared_network" => Ok(ThreadingMode::SharedNetwork),
        "shared" => Ok(ThreadingMode::Shared),
        "invoker" => Ok(ThreadingMode::Invoker),
        _ => Err(format!(
            "Unknown threading mode: '{}'. Expected: dedicated, shared_network, shared, invoker",
            s
        )),
    }
}

fn parse_idle_strategy(s: &str) -> Result<IdleStrategy, String> {
    match s {
        "backoff" => Ok(IdleStrategy::Backoff),
        "spin" => Ok(IdleStrategy::Spin),
        "yield" => Ok(IdleStrategy::Yield),
        "sleeping" => Ok(IdleStrategy::Sleeping),
        "noop" => Ok(IdleStrategy::Noop),
        _ => Err(format!(
            "Unknown idle strategy: '{}'. Expected: backoff, spin, yield, sleeping, noop",
            s
        )),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = std::env::args().nth(1);

    let config = if let Some(ref path) = config_path {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {}", path, e))?;
        let cfg: Config = serde_yaml::from_str(&contents)
            .map_err(|e| format!("Failed to parse config file '{}': {}", path, e))?;
        println!("Loaded config from: {}", path);
        cfg
    } else {
        println!("No config file specified, using Aeron defaults.");
        Config::default()
    };

    println!("Starting Aeron Media Driver...");

    let mut driver = MediaDriver::new()?;

    // Apply configuration
    if let Some(ref dir) = config.dir {
        driver.set_dir(dir)?;
    }
    if let Some(v) = config.dir_delete_on_start {
        driver.set_dir_delete_on_start(v)?;
    }
    if let Some(v) = config.dir_delete_on_shutdown {
        driver.set_dir_delete_on_shutdown(v)?;
    }
    if let Some(ref mode) = config.threading_mode {
        driver.set_threading_mode(parse_threading_mode(mode)?)?;
    }
    if let Some(ref s) = config.conductor_idle_strategy {
        driver.set_conductor_idle_strategy(parse_idle_strategy(s)?)?;
    }
    if let Some(ref s) = config.sender_idle_strategy {
        driver.set_sender_idle_strategy(parse_idle_strategy(s)?)?;
    }
    if let Some(ref s) = config.receiver_idle_strategy {
        driver.set_receiver_idle_strategy(parse_idle_strategy(s)?)?;
    }
    if let Some(v) = config.term_buffer_length {
        driver.set_term_buffer_length(v)?;
    }
    if let Some(v) = config.ipc_term_buffer_length {
        driver.set_ipc_term_buffer_length(v)?;
    }
    if let Some(v) = config.mtu_length {
        driver.set_mtu_length(v)?;
    }
    if let Some(v) = config.ipc_mtu_length {
        driver.set_ipc_mtu_length(v)?;
    }
    if let Some(v) = config.socket_so_rcvbuf {
        driver.set_socket_so_rcvbuf(v)?;
    }
    if let Some(v) = config.socket_so_sndbuf {
        driver.set_socket_so_sndbuf(v)?;
    }
    if let Some(v) = config.print_configuration {
        driver.set_print_configuration(v)?;
    }
    if let Some(v) = config.conductor_cpu_affinity {
        driver.set_conductor_cpu_affinity(v)?;
    }
    if let Some(v) = config.sender_cpu_affinity {
        driver.set_sender_cpu_affinity(v)?;
    }
    if let Some(v) = config.receiver_cpu_affinity {
        driver.set_receiver_cpu_affinity(v)?;
    }

    driver.start()?;
    println!("Media Driver started successfully.");
    println!("Press Ctrl+C to shut down...");

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        println!("\nShutting down Media Driver...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }

    println!("Media Driver stopped.");
    Ok(())
}
