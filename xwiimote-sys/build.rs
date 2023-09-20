use bindgen::CargoCallbacks;
use std::env;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
fn main() {
    println!("cargo:rustc-link-lib=udev");
    println!("cargo:rerun-if-changed=vendor/lib");
    println!("cargo:rerun-if-changed=wrapper.h");

    build_xwiimote();

    // Generate the Rust FFI bindings to the xwiimote library.
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .allowlist_type("xwii_.*")
        .allowlist_function("xwii_.*")
        .allowlist_var("XWII_.*")
        .derive_default(true)
        .prepend_enum_name(false)
        // Tell cargo to invalidate the built crate whenever any
        // of the included header files changes.
        .parse_callbacks(Box::new(CargoCallbacks))
        .generate()
        .expect("unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write bindings");
}

fn build_xwiimote() {
    println!("cargo:rerun-if-env-changed=XWIIMOTE_SYS_STATIC");
    let want_static =
        cfg!(feature = "static") || env::var("XWIIMOTE_SYS_STATIC").unwrap_or(String::new()) == "1";
    if !want_static {
        // Run pkg-config since we're linking dynamically.
        let xwiimote = pkg_config::Config::new()
            .atleast_version("2")
            .probe("libxwiimote");
        match xwiimote {
            Ok(_) => return,
            Err(e) => {
                // Couldn't locate the library; fall back to static build.
                println!("cargo-warning={}", e.to_string());
            }
        }
    }

    // Compile the source files into a static library.
    cc::Build::new()
        .define("XWII__EXPORT", r#"__attribute__((visibility("default")))"#)
        .file("vendor/lib/core.c")
        .file("vendor/lib/monitor.c")
        // The unused enum-array entries are initialized to -1 using
        // the designated initializer [0 ... MAX] = -1, which causes
        // a double initialization when the entry of each enum variant
        // is initialized. This is mostly harmless, so we ignore it.
        .flag("-Wno-override-init")
        .compile("xwiimote");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("Cannot build xwiimote on non-Linux system");
}
