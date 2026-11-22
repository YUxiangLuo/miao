use super::EmbeddedKernel;

macro_rules! embedded_kernel {
    ($name:literal) => {
        EmbeddedKernel {
            compressed: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../embedded/",
                $name,
                ".zst"
            )),
            metadata: include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../embedded/",
                $name,
                ".meta.json"
            )),
        }
    };
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-windows-amd64.exe");

#[cfg(all(windows, target_arch = "aarch64"))]
compile_error!("Windows arm64 is not supported yet");

#[cfg(all(not(windows), target_arch = "x86_64"))]
pub(super) const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-amd64");

#[cfg(all(not(windows), target_arch = "aarch64"))]
pub(super) const KERNEL: EmbeddedKernel = embedded_kernel!("sing-box-arm64");

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("Unsupported architecture: only x86_64 and aarch64 are supported");

pub(super) const IP_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geoip-cn.srs"
));
pub(super) const SITE_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geosite-geolocation-cn.srs"
));
