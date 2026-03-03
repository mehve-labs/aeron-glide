use cmake::Config;
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=src/shim.h");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Configurable Aeron version
    let aeron_version = env::var("AERON_VERSION").unwrap_or_else(|_| "1.50.2".to_string());
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

    // Build Aeron C++ using CMake
    let mut config = Config::new(&aeron_dir);
    config
        .define("BUILD_AERON_DRIVER", "ON")
        .define("BUILD_AERON_ARCHIVE_API", "OFF")
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

    // Build the cxx bridge
    cxx_build::bridge("src/lib.rs")
        .file("src/shim.cc")
        .include(include_path)
        .include(c_client_include_path)
        .include(driver_include_path)
        .include("src")
        .flag_if_supported("-std=c++14")
        .flag_if_supported("-Wno-unused-parameter")
        .compile("aeron_rs_cxx");
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
