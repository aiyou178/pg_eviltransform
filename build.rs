fn main() {
    println!("cargo:rerun-if-changed=src/rtgeom_fast.c");

    if std::env::var_os("CARGO_FEATURE_EXTENSION").is_none() {
        return;
    }

    cc::Build::new()
        .file("src/rtgeom_fast.c")
        .flag_if_supported("-O3")
        .compile("pgct_rtgeom_fast");

    println!("cargo:rustc-link-lib=rttopo");
    println!("cargo:rustc-link-lib=m");
}
