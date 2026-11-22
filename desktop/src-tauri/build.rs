fn main() {
    let static_vc_runtime =
        std::env::var_os("STATIC_VCRUNTIME").is_none_or(|value| value != "false");
    std::env::remove_var("STATIC_VCRUNTIME");
    let attributes = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().static_vc_runtime(static_vc_runtime),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
