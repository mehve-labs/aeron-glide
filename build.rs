use cmake::Config;
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=src/shim.h");

    // docs.rs builds in a network-isolated sandbox and only runs `cargo doc`,
    // which compiles the crate but never links. Skip the Aeron download, the
    // CMake build, and the cxx C++ compilation entirely — the cxx bridge still
    // expands to pure-Rust FFI declarations, so rustdoc succeeds without them.
    if env::var("DOCS_RS").is_ok() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Configurable Aeron version
    let aeron_version = env::var("AERON_VERSION").unwrap_or_else(|_| "1.52.0".to_string());
    println!("cargo:rerun-if-env-changed=AERON_VERSION");

    let aeron_dir = out_dir.join(format!("aeron-{}", aeron_version));

    // Download and extract Aeron if it doesn't exist
    if !aeron_dir.exists() {
        let url = format!(
            "https://github.com/real-logic/aeron/archive/refs/tags/{}.tar.gz",
            aeron_version
        );
        download_and_extract(&url, &out_dir);
    }

    let archive_enabled = env::var("CARGO_FEATURE_ARCHIVE").is_ok();

    // Build Aeron C++ using CMake
    let mut config = Config::new(&aeron_dir);
    config
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

    println!("cargo:rustc-link-lib=static=aeron_static");
    println!("cargo:rustc-link-lib=static=aeron_driver_static");

    if archive_enabled {
        println!("cargo:rustc-link-lib=static=aeron_archive_c_client_static");
    }

    // OS specific dependencies
    if cfg!(target_os = "windows") {
        println!("cargo:rustc-link-lib=shell32");
        println!("cargo:rustc-link-lib=iphlpapi");
    }
    if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=uuid");
        println!("cargo:rustc-link-lib=bsd");
    }

    let include_path = aeron_dir.join("aeron-client/src/main/cpp_wrapper");
    let c_client_include_path = aeron_dir.join("aeron-client/src/main/c");
    let driver_include_path = aeron_dir.join("aeron-driver/src/main/c");

    // Build the cxx bridge(s)
    let mut bridge_sources: Vec<&str> = vec!["src/lib.rs"];
    if archive_enabled {
        bridge_sources.push("src/archive.rs");
        println!("cargo:rerun-if-changed=src/archive.rs");
    }

    let mut builder = cxx_build::bridges(bridge_sources);
    builder
        .file("src/shim.cc")
        .include(&include_path)
        .include(&c_client_include_path)
        .include(&driver_include_path)
        .include("src")
        .flag_if_supported("-std=c++14")
        .flag_if_supported("-Wno-unused-parameter");

    if archive_enabled {
        let archive_cpp_path = aeron_dir.join("aeron-archive/src/main/cpp_wrapper");
        let archive_c_path = aeron_dir.join("aeron-archive/src/main/c");
        builder
            .include(&archive_cpp_path)
            .include(&archive_c_path)
            .define("AERON_ARCHIVE", None);
    }

    builder.compile("aeron_rs_cxx");
}

fn download_and_extract(url: &str, dest_dir: &PathBuf) {
    println!("cargo:warning=Downloading Aeron source from {}", url);
    let response = reqwest::blocking::get(url).expect("Failed to download Aeron");
    let bytes = response.bytes().expect("Failed to read response bytes");
    let cursor = std::io::Cursor::new(bytes);

    let decoder = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest_dir)
        .expect("Failed to unpack Aeron archive");
}
