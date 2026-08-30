fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    println!("cargo:rerun-if-env-changed=RUSTDOCFLAGS");
    for (source, destination) in [
        ("TARGET", "TOMLSMITH_BUILD_TARGET"),
        ("PROFILE", "TOMLSMITH_BUILD_PROFILE"),
        ("CARGO_ENCODED_RUSTFLAGS", "TOMLSMITH_BUILD_ENCODED_RUSTFLAGS"),
        ("RUSTDOCFLAGS", "TOMLSMITH_BUILD_RUSTDOCFLAGS"),
    ] {
        let value = std::env::var(source).unwrap_or_default();
        println!("cargo:rustc-env={destination}={value}");
    }
}
