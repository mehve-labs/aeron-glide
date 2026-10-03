use cmake::Config;
use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=src/shim.h");
    println!("cargo:rerun-if-changed=src/counter_types.h");

    // docs.rs builds in a network-isolated sandbox and only runs `cargo doc`,
    // which compiles the crate but never links. Skip the Aeron download, the
    // CMake build, and the cxx C++ compilation entirely — the cxx bridge still
    // expands to pure-Rust FFI declarations, so rustdoc succeeds without them.
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Configurable Aeron version. The generated src/driver_gen.{rs,h} target the
    // default; other versions may need `scripts/gen_driver_context.py` rerun.
    let aeron_version = env::var("AERON_VERSION").unwrap_or_else(|_| "1.53.3".to_string());
    println!("cargo:rerun-if-env-changed=AERON_VERSION");

    let aeron_dir = aeron_source(&aeron_version, &out_dir);

    let archive_enabled = env::var("CARGO_FEATURE_ARCHIVE").is_ok();

    // Build Aeron C++ using CMake
    let mut config = Config::new(&aeron_dir);
    config
        // Aeron's CMake stamps `git log` of its source directory into the driver
        // (e.g. the "Aeron software" counter label). The extracted tarball is not a
        // repository, so stop git from finding the enclosing project's instead.
        .env("GIT_CEILING_DIRECTORIES", &out_dir)
        .define("BUILD_AERON_DRIVER", "ON")
        .define(
            "BUILD_AERON_ARCHIVE_API",
            if archive_enabled { "ON" } else { "OFF" },
        )
        .define("AERON_TESTS", "OFF")
        .define("AERON_BUILD_SAMPLES", "OFF")
        .define("AERON_BUILD_DOCUMENTATION", "OFF");

    if env::var("PROFILE").unwrap() == "release" {
        config.profile("Release");
    } else {
        config.profile("Debug");
    }

    let cmake_output = config.build();
    let base_lib_dir = cmake_output.join("build");

    // Add search paths for linker
    println!(
        "cargo:rustc-link-search=native={}",
        base_lib_dir.join("lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        base_lib_dir.join("lib/Debug").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        base_lib_dir.join("lib/Release").display()
    );

    let include_path = aeron_dir.join("aeron-client/src/main/cpp_wrapper");
    let c_client_include_path = aeron_dir.join("aeron-client/src/main/c");
    let driver_include_path = aeron_dir.join("aeron-driver/src/main/c");

    // Build the cxx bridge(s)
    let mut bridge_sources: Vec<&str> = vec!["src/lib.rs", "src/driver_gen.rs"];
    println!("cargo:rerun-if-changed=src/driver_gen.rs");
    println!("cargo:rerun-if-changed=src/driver_gen.h");
    if archive_enabled {
        bridge_sources.push("src/archive/mod.rs");
        for file in [
            "src/archive/mod.rs",
            "src/archive_shim.h",
            "src/archive_shim.cc",
        ] {
            println!("cargo:rerun-if-changed={file}");
        }
    }

    let mut builder = cxx_build::bridges(bridge_sources);
    builder
        .file("src/shim.cc")
        .include(&include_path)
        .include(&c_client_include_path)
        .include(&driver_include_path)
        .include("src")
        // C++17 for guaranteed copy elision: aeron::CncFileReader is copyable but
        // closes its mapping in its destructor, so it must never be copied.
        .flag_if_supported("-std=c++17")
        .flag_if_supported("/std:c++17")
        .flag_if_supported("-Wno-unused-parameter");

    if archive_enabled {
        builder.file("src/archive_shim.cc");
        let archive_cpp_path = aeron_dir.join("aeron-archive/src/main/cpp_wrapper");
        let archive_c_path = aeron_dir.join("aeron-archive/src/main/c");
        builder
            .include(&archive_cpp_path)
            .include(&archive_c_path)
            .define("AERON_ARCHIVE", None);
    }

    builder.compile("aeron_rs_cxx");

    // After the shim library (emitted by `compile`), in dependency order: GNU ld
    // resolves static libraries left to right.
    if archive_enabled {
        println!("cargo:rustc-link-lib=static=aeron_archive_c_client_static");
    }
    println!("cargo:rustc-link-lib=static=aeron_driver_static");
    println!("cargo:rustc-link-lib=static=aeron_static");

    // OS-specific dependencies of the target (not the host running build.rs).
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => {
            println!("cargo:rustc-link-lib=shell32");
            println!("cargo:rustc-link-lib=iphlpapi");
        }
        Ok("linux") => {
            println!("cargo:rustc-link-lib=uuid");
            println!("cargo:rustc-link-lib=bsd");
        }
        _ => {}
    }
}

/// SHA-256 of the GitHub source tarball of each supported Aeron version.
const AERON_SHA256: &[(&str, &str)] = &[(
    "1.53.3",
    "b7861c4aa9bd4918c0c3cb3b83b4a631cf48c1403e496008ccbcb950a121f125",
)];

/// The Aeron source tree: `AERON_SOURCE_DIR` if set (offline builds), otherwise
/// the release tarball, downloaded once into `OUT_DIR` and checked against its
/// SHA-256 (`AERON_SHA256` overrides the expected hash, e.g. for another
/// `AERON_VERSION`).
fn aeron_source(version: &str, out_dir: &Path) -> PathBuf {
    println!("cargo:rerun-if-env-changed=AERON_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=AERON_SHA256");
    if let Some(dir) = env::var_os("AERON_SOURCE_DIR") {
        let dir = PathBuf::from(dir);
        assert!(
            dir.join("CMakeLists.txt").exists(),
            "AERON_SOURCE_DIR={} is not an Aeron source tree (no CMakeLists.txt)",
            dir.display()
        );
        return dir;
    }
    let aeron_dir = out_dir.join(format!("aeron-{version}"));
    // Written last: a tree without it is a partial extract, e.g. of an
    // interrupted build.
    let complete = out_dir.join(format!("aeron-{version}.complete"));
    if aeron_dir.exists() && complete.exists() {
        return aeron_dir;
    }

    let url = format!("https://github.com/real-logic/aeron/archive/refs/tags/{version}.tar.gz");
    println!("cargo:warning=Downloading Aeron source from {url}");
    let tarball = download(&url);

    let expected = env::var("AERON_SHA256").ok().or_else(|| {
        AERON_SHA256
            .iter()
            .find(|(v, _)| *v == version)
            .map(|(_, sha)| sha.to_string())
    });
    let actual = sha256_hex(&tarball);
    match expected {
        Some(expected) => assert!(
            actual.eq_ignore_ascii_case(expected.trim()),
            "the Aeron {version} tarball from {url} has SHA-256 {actual}, expected {expected}; \
             set AERON_SHA256 to accept it, or AERON_SOURCE_DIR to build from a local source tree"
        ),
        None => println!(
            "cargo:warning=No known SHA-256 for Aeron {version} (downloaded {actual}); set AERON_SHA256 to verify it"
        ),
    }

    // Extract next to the final directory, then move it into place.
    let staging = out_dir.join(format!("aeron-{version}.extracting"));
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&aeron_dir);
    let _ = std::fs::remove_file(&complete);
    tar::Archive::new(flate2::read::GzDecoder::new(tarball.as_slice()))
        .unpack(&staging)
        .expect("Failed to unpack the Aeron tarball");
    std::fs::rename(staging.join(format!("aeron-{version}")), &aeron_dir)
        .expect("Unexpected layout of the Aeron tarball");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::write(&complete, &actual).expect("Failed to mark the Aeron source as complete");
    aeron_dir
}

fn download(url: &str) -> Vec<u8> {
    let mut response = ureq::get(url)
        .call()
        .unwrap_or_else(|e| panic!("Failed to download {url}: {e}"));
    response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .unwrap_or_else(|e| panic!("Failed to download {url}: {e}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
