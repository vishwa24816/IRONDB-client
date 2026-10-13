fn main() {
    tauri_build::build();

    // Test binaries link wry/tao (TaskDialogIndirect from comctl32 v6) but get
    // no v6 manifest — tauri-build embeds one only in the app binary. Without
    // it, `cargo test -p irondb-tauri` dies at load with
    // STATUS_ENTRYPOINT_NOT_FOUND. Embed the manifest in test targets only.
    // ponytail: windres + link-arg-tests; revisit if toolchain goes MSVC-only.
    println!("cargo::rerun-if-changed=test-exe.rc");
    println!("cargo::rerun-if-changed=test-exe.manifest");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let obj = format!("{out}/test-exe-manifest.o");
    let rc = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-exe.rc");
    let status = std::process::Command::new("windres")
        .args([
            rc.to_str().expect("rc path"),
            "--output-format=coff",
            "-o",
            &obj,
        ])
        .status();
    match status {
        Ok(s) if s.success() => {
            // tests/: integration suites. lib-unittests are NOT covered by
            // link-arg-tests in this toolchain, so also pass it blanket (the
            // app bin already carries tauri-build's own manifest; an extra
            // identical comctl dependency is benign).
            println!("cargo::rustc-link-arg-tests={obj}");
            println!("cargo::rustc-link-arg={obj}");
        }
        other => println!("cargo::warning=windres failed ({other:?}); tauri tests may not start"),
    }
}
