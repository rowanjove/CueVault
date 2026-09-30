fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest_dir.join("..").join("提示词管理.html");
    let dst = manifest_dir.join("..").join("ui").join("index.html");
    std::fs::copy(&src, &dst).expect("同步界面文件失败");
    tauri_build::build();
}
