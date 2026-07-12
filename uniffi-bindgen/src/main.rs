//! Workspace-local `uniffi-bindgen` binary.
//!
//! UniFFI 0.28 ships its bindgen as the `uniffi_bindgen` *library* crate with
//! no published CLI binary on crates.io (and 0.28 has no `cli` feature, unlike
//! 0.31+), so we drive the library-mode generator directly.
//!
//! Usage: `cargo run -p uniffi-bindgen -- <path/to/lib.so> <out_dir>`
//! (Kotlin is hard-coded — this workspace has one foreign target.)

use camino::Utf8PathBuf;
use uniffi_bindgen::{
    EmptyCrateConfigSupplier, bindings::KotlinBindingGenerator, library_mode,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        let prog = args.first().map(String::as_str).unwrap_or("uniffi-bindgen");
        anyhow::bail!("usage: {prog} <path/to/libfoo.so> <out_dir>");
    }
    let library = Utf8PathBuf::from(&args[1]);
    let out_dir = Utf8PathBuf::from(&args[2]);
    library_mode::generate_bindings(
        library.as_path(),
        None,
        &KotlinBindingGenerator,
        &EmptyCrateConfigSupplier,
        None,
        out_dir.as_path(),
        true,
    )?;
    println!("uniffi-bindgen: wrote Kotlin bindings for {} -> {}", library, out_dir);
    Ok(())
}
