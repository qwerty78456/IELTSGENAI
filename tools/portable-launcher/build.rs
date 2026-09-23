fn main() {
    println!("cargo:rerun-if-env-changed=IELTS_PORTABLE_PAYLOAD");
    let path = std::env::var("IELTS_PORTABLE_PAYLOAD")
        .expect("Set IELTS_PORTABLE_PAYLOAD to the packaged ZIP");
    println!("cargo:rerun-if-changed={path}");
    println!("cargo:rustc-env=IELTS_PORTABLE_PAYLOAD={path}");
}
