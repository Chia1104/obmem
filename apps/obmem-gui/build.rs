//! Reads `locales/` as the nsh CLI does, so that a build by Cargo alone (`cargo run`, the tests)
//! has the same translations as `nsh run` and `nsh package`, which read the folder themselves.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=locales");
    let app = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let translations =
        nsh_i18n::read_translations(Path::new(&app)).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(
        Path::new(&out).join("translations.json"),
        nsh_i18n::translations_json(&translations),
    )
    .expect("the translations are written to OUT_DIR");
}
