//! An Aeron Archive for tests: the Java `ArchivingMediaDriver` from the
//! `aeron-all` jar built with the `archive` feature, in its own directory.
//!
//! The jar is found under `target/` (or set `AERON_ALL_JAR`). Without Java or the jar, `ArchiveDriver::start` returns `None` and the test
//! is skipped, unless `AERON_GLIDE_REQUIRE_ARCHIVE` is set.

use super::{TIMEOUT, free_udp_port};
use aeron_glide::archive::{self, AeronArchive};
use aeron_glide::{AeronClient, CncFile, Context};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const CONTROL_STREAM_ID: i32 = 10;
pub const RESPONSE_STREAM_ID: i32 = 20;

pub struct ArchiveDriver {
    child: Child,
    pub dir: PathBuf,
    /// The media driver's Aeron directory.
    pub aeron_dir: String,
    pub control_channel: String,
    pub archive_id: i64,
}

fn jar() -> Option<PathBuf> {
    if let Some(jar) = std::env::var_os("AERON_ALL_JAR") {
        return Some(PathBuf::from(jar));
    }
    let version = std::env::var("AERON_VERSION").unwrap_or_else(|_| "1.53.3".to_string());
    let jar_path = format!("aeron-all/build/libs/aeron-all-{version}.jar");
    // Built inside the source tree when building from AERON_SOURCE_DIR.
    if let Some(source) = std::env::var_os("AERON_SOURCE_DIR") {
        let jar = PathBuf::from(source).join(&jar_path);
        if jar.exists() {
            return Some(jar);
        }
    }
    let file = format!("out/aeron-{version}/{jar_path}");
    // Otherwise it is in the build script's output directory of the profile this
    // test binary was built with (which may be outside the repository, e.g.
    // CARGO_TARGET_DIR): build/<pkg>-<hash>/out, or build/<pkg>/<hash>/out in
    // newer Cargo layouts, which also put test binaries under build/. The
    // profile directory is the nearest ancestor of the binary whose build/
    // holds this crate's outputs; look there first, then in the other profiles
    // next to it.
    let exe = std::env::current_exe().ok()?;
    let ours = |dir: &Path| {
        std::fs::read_dir(dir.join("build")).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("aeron-glide"))
        })
    };
    let profile = exe.ancestors().skip(1).take(5).find(|dir| ours(dir))?;
    let mut builds = vec![profile.join("build")];
    if let Some(Ok(profiles)) = profile.parent().map(std::fs::read_dir) {
        builds.extend(profiles.flatten().map(|p| p.path().join("build")));
    }
    for build in builds {
        let Ok(entries) = std::fs::read_dir(&build) else {
            continue;
        };
        for entry in entries.flatten() {
            let mut dirs = vec![entry.path()];
            if let Ok(nested) = std::fs::read_dir(entry.path()) {
                dirs.extend(nested.flatten().map(|n| n.path()));
            }
            if let Some(jar) = dirs.iter().map(|d| d.join(&file)).find(|j| j.exists()) {
                return Some(jar);
            }
        }
    }
    eprintln!("no {file} near {}", exe.display());
    None
}

impl ArchiveDriver {
    pub fn start() -> Option<Self> {
        Self::start_with(&[])
    }

    /// Start with extra `-D` system properties.
    pub fn start_with(properties: &[(&str, &str)]) -> Option<Self> {
        let required = std::env::var_os("AERON_GLIDE_REQUIRE_ARCHIVE").is_some();
        let Some(jar) = jar() else {
            assert!(!required, "aeron-all jar not found");
            eprintln!("skipping: the aeron-all jar was not built");
            return None;
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("aeron-glide-archive-{}-{n}", std::process::id()));
        let aeron_dir = dir.join("driver").to_string_lossy().into_owned();
        let control_channel = format!("aeron:udp?endpoint=localhost:{}", free_udp_port());
        let archive_id = i64::from(std::process::id()) * 1000 + n as i64;
        let mut props: Vec<(String, String)> = [
            ("aeron.dir", aeron_dir.as_str()),
            ("aeron.dir.delete.on.start", "true"),
            ("aeron.dir.delete.on.shutdown", "true"),
            ("aeron.threading.mode", "SHARED"),
            ("aeron.term.buffer.length", "65536"),
            ("aeron.ipc.term.buffer.length", "65536"),
            ("aeron.publication.linger.timeout", "50ms"),
            ("aeron.archive.threading.mode", "SHARED"),
            ("aeron.archive.dir.delete.on.start", "true"),
            ("aeron.archive.control.channel", control_channel.as_str()),
            (
                "aeron.archive.replication.channel",
                "aeron:udp?endpoint=localhost:0",
            ),
            ("aeron.archive.segment.file.length", "262144"),
            ("aeron.archive.recording.events.enabled", "false"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        props.push((
            "aeron.archive.dir".into(),
            dir.join("archive").to_string_lossy().into_owned(),
        ));
        props.push(("aeron.archive.id".into(), archive_id.to_string()));
        props.push((
            "aeron.archive.control.stream.id".into(),
            CONTROL_STREAM_ID.to_string(),
        ));
        for (k, v) in properties {
            props.push((k.to_string(), v.to_string()));
        }
        let mut command = Command::new("java");
        command
            .arg("--add-opens")
            .arg("java.base/jdk.internal.misc=ALL-UNNAMED")
            .arg("--add-opens")
            .arg("java.base/sun.nio.ch=ALL-UNNAMED");
        for (k, v) in &props {
            command.arg(format!("-D{k}={v}"));
        }
        let child = command
            .arg("-cp")
            .arg(&jar)
            .arg("io.aeron.archive.ArchivingMediaDriver")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(e) => {
                assert!(!required, "cannot run java: {e}");
                eprintln!("skipping: cannot run java: {e}");
                return None;
            }
        };
        let driver = Self {
            child,
            dir,
            aeron_dir,
            control_channel,
            archive_id,
        };
        // Wait for the media driver, then for the archive to answer.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Ok(cnc) = CncFile::map_existing_with_timeout(&driver.aeron_dir, Duration::ZERO)
                && cnc.is_driver_active(Duration::from_secs(10))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the archiving media driver did not start"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut client = driver.client();
        loop {
            if client.is_closed() {
                // E.g. timed out while the JVM was busy starting.
                client = driver.client();
            }
            match driver
                .context(&client)
                .message_timeout(Duration::from_secs(1))
                .connect()
            {
                Ok(_) => break,
                Err(e) => assert!(Instant::now() < deadline, "the archive did not start: {e}"),
            }
        }
        Some(driver)
    }

    /// A client of the archive's media driver.
    pub fn client(&self) -> AeronClient {
        AeronClient::connect(Context::new().aeron_dir(&self.aeron_dir)).expect("connect client")
    }

    /// An archive context for this archive, using `client`.
    pub fn context(&self, client: &AeronClient) -> archive::Context {
        archive::Context::new()
            .aeron(client)
            .control_request_channel(&self.control_channel)
            .control_request_stream_id(CONTROL_STREAM_ID)
            .control_response_channel("aeron:udp?endpoint=localhost:0")
            .control_response_stream_id(RESPONSE_STREAM_ID)
            .message_timeout(TIMEOUT)
    }

    /// Connect to this archive with `client`.
    pub fn connect(&self, client: &AeronClient) -> AeronArchive {
        self.context(client)
            .connect()
            .expect("connect to the archive")
    }
}

impl Drop for ArchiveDriver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Start an archive or skip the test.
#[macro_export]
macro_rules! archive_or_skip {
    () => {
        match common::archive::ArchiveDriver::start() {
            Some(archive) => archive,
            None => return,
        }
    };
}
