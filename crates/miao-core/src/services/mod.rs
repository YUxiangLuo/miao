pub mod config;
pub mod mcp;
pub mod node_parser;
#[cfg(not(windows))]
pub mod openwrt;
pub mod proxy;
pub mod singbox;
pub mod status;
pub mod subscription;
pub mod version;
#[cfg(not(windows))]
pub mod vps;
