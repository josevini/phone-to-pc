//! The Kotlin bindings generate from the built library, as the Android build does.

use std::path::Path;
use std::process::Command;

#[test]
fn kotlin_bindings_generate_from_the_library() {
    let bindgen = Path::new(env!("CARGO_BIN_EXE_uniffi-bindgen"));
    // Cargo builds the cdylib into `deps/` next to the binaries (and only copies it up for plain builds).
    let library = bindgen.with_file_name("deps").join("libclipsync_ffi.so");
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(bindgen)
        .args(["generate", "--no-format", "--language", "kotlin", "--library"])
        .arg(&library)
        .arg("--out-dir")
        .arg(out.path())
        .status()
        .unwrap();
    assert!(status.success());
    let kotlin = std::fs::read_to_string(out.path().join("io/github/josevini/clipsync/core/clipsync_ffi.kt")).unwrap();
    assert!(kotlin.contains("package io.github.josevini.clipsync.core"));
    assert!(kotlin.contains("open class Engine"));
    assert!(kotlin.contains("fun `parsePairUri`(`uri`: kotlin.String): PairUri"));
}
