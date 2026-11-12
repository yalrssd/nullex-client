fn main() {
    if std::path::Path::new("../dist").exists() {
        println!("cargo:rerun-if-changed=../dist");
        println!("cargo:rerun-if-changed=../dist/index.html");
        println!("cargo:rerun-if-changed=../dist/assets");
    }
    tauri_build::build()
}
