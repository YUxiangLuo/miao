pub mod clash;
pub mod mcp;
pub mod nodes;
pub mod proxy;
pub mod rules;
pub mod service;
#[cfg_attr(
    not(feature = "embedded-assets"),
    path = "static_assets_unavailable.rs"
)]
pub mod static_assets;
pub mod subs;
pub mod version;

pub mod vps;
