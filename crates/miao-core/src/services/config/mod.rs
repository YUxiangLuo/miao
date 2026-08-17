mod apply;
mod builder;
mod generate;
mod persist;
mod region;

#[cfg(test)]
mod tests;

pub use apply::{
    apply_config_change, refresh_subscriptions, regenerate_preserving_service_state, RefreshEffect,
    RefreshPolicy, SubSource,
};
pub use generate::{
    gen_config, known_rule_targets, record_fresh_snapshot, GenConfigOutcome, SubFetchRetry,
};
pub use persist::{
    has_config_cache, load_volatile_config, persist_effective_node_select,
    restore_config_from_cache, save_config_cache, save_config_to, volatile_config_path,
};
pub use region::runtime_config_matches_node_select;
