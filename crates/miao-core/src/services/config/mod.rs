mod apply;
mod bindings;
mod builder;
mod generate;
mod persist;
mod region;
mod warnings;

#[cfg(test)]
mod tests;

pub use apply::{
    apply_config_change, apply_disabled_nodes, apply_max_multiplier, apply_node_select,
    apply_route_mode, edit_subscriptions, install_prepared_runtime, refresh_subscriptions,
    refresh_subscriptions_foreground, ConfigMutationError, RefreshEffect, RefreshPolicy,
    RuntimeUpdate, SubSource,
};
pub use generate::{
    collect_manual_outbounds, fetch_sub_nodes_if_current, gen_config_from_nodes,
    known_rule_targets, publish_generation_diagnostics, publish_runtime_multiplier_options,
    subscription_source_id, GenConfigOutcome, SubFetchRetry,
};
pub use persist::{
    cache_compatibility, has_config_cache, load_max_multiplier_preference,
    load_node_select_preference, load_volatile_config_at, mark_legacy_cache_used,
    persist_effective_node_select, read_sub_nodes_snapshot, restore_config_from_cache,
    save_config_cache, save_max_multiplier_preference, save_node_select_preference,
    save_stable_fields, volatile_config_path, CacheCompatibility, SubNodesReadModel,
};
#[cfg(test)]
pub use persist::{save_sub_nodes_snapshot, SubNodesSnapshot};
pub use region::runtime_config_matches_node_select;
pub use warnings::{
    ALL_SUBS_FAILED_KEEP_CACHE, ALL_SUBS_FAILED_RETRY, DATA_PLANE_RETRYING,
    REFRESH_FAILED_KEEP_CACHE, REFRESH_VALIDATION_FAILED, REGION_FALLBACK,
    STARTUP_VALIDATION_RETRY, SUBS_REFRESHING_MANUAL,
};

#[cfg(test)]
pub use apply::regenerate_preserving_service_state;

pub(crate) use persist::write_file_atomic;
