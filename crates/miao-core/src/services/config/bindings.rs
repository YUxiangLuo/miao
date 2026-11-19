use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::builder::make_unique_tag;
use super::generate::FetchedNode;
use super::persist::write_file_atomic;

const BINDINGS_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeTagBinding {
    pub stable_key: String,
    pub content_key: String,
    pub affinity_key: String,
    pub tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NodeTagBindings {
    pub version: u32,
    pub bindings: Vec<NodeTagBinding>,
    /// Binds local subscription material to the accepted runtime transaction.
    /// An old snapshot left by a best-effort write failure must not be reused
    /// after restart. None preserves compatibility with pre-fingerprint files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_snapshot_sha256: Option<String>,
}

impl Default for NodeTagBindings {
    fn default() -> Self {
        Self {
            version: BINDINGS_VERSION,
            bindings: Vec::new(),
            subscription_snapshot_sha256: None,
        }
    }
}

fn sha256_parts(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hex::encode(hasher.finalize())
}

fn outbound_without_tag(outbound: &serde_json::Value) -> serde_json::Value {
    let mut value = outbound.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("tag");
    }
    value
}

fn outbound_bytes(outbound: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&outbound_without_tag(outbound)).unwrap_or_default()
}

fn stable_key(source: &str, bytes: &[u8], duplicate_index: usize) -> String {
    sha256_parts(&[
        source.as_bytes(),
        bytes,
        duplicate_index.to_string().as_bytes(),
    ])
}

fn content_key(bytes: &[u8], duplicate_index: usize) -> String {
    sha256_parts(&[bytes, duplicate_index.to_string().as_bytes()])
}

fn affinity_key(node: &FetchedNode) -> String {
    let value = serde_json::json!({
        "type": node.outbound.get("type"),
        "server": node.outbound.get("server"),
        "server_port": node.outbound.get("server_port"),
        "uuid": node.outbound.get("uuid"),
        "username": node.outbound.get("username"),
    });
    let bytes = serde_json::to_vec(&value).unwrap_or_default();
    sha256_parts(&[&bytes])
}

pub(super) async fn load_bindings(state: &AppState) -> AppResult<Option<NodeTagBindings>> {
    let bytes = match tokio::fs::read(&state.runtime_paths.node_bindings).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::context(
                "Failed to read stable node tag bindings",
                err,
            ));
        }
    };
    let parsed: NodeTagBindings = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::context("Stable node tag bindings are invalid", e))?;
    if parsed.version != BINDINGS_VERSION {
        return Err(AppError::message(format!(
            "Unsupported stable node tag bindings version {}",
            parsed.version
        )));
    }
    Ok(Some(parsed))
}

pub async fn reserved_node_tags(state: &AppState) -> Vec<String> {
    match load_bindings(state).await {
        Ok(Some(bindings)) => bindings
            .bindings
            .into_iter()
            .map(|binding| binding.tag)
            .collect(),
        Ok(None) => Vec::new(),
        Err(err) => {
            warn!(error = %err, "Ignoring unusable stable node tag bindings");
            Vec::new()
        }
    }
}

async fn active_tag_by_fingerprint(state: &AppState) -> HashMap<Vec<u8>, Vec<String>> {
    let bytes = match tokio::fs::read(&state.runtime_paths.active_config).await {
        Ok(bytes) => bytes,
        Err(_) => return HashMap::new(),
    };
    let config: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return HashMap::new(),
    };
    let mut result: HashMap<Vec<u8>, Vec<String>> = HashMap::new();
    for outbound in config["outbounds"].as_array().into_iter().flatten() {
        let Some(tag) = outbound.get("tag").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if matches!(tag, "proxy" | "direct") {
            continue;
        }
        result
            .entry(outbound_bytes(outbound))
            .or_default()
            .push(tag.to_string());
    }
    result
}

struct PreparedNodeIdentity {
    node: FetchedNode,
    fingerprint: Vec<u8>,
    stable_key: String,
    content_key: String,
    affinity_key: String,
}

fn prepare_node_identities(nodes: Vec<FetchedNode>) -> Vec<PreparedNodeIdentity> {
    let mut fingerprint_occurrences: HashMap<(String, Vec<u8>), usize> = HashMap::new();
    let mut prepared = Vec::with_capacity(nodes.len());
    for node in nodes {
        let fingerprint = outbound_bytes(&node.outbound);
        let occurrence = fingerprint_occurrences
            .entry((node.source_id.clone(), fingerprint.clone()))
            .or_default();
        let stable_key = stable_key(&node.source_id, &fingerprint, *occurrence);
        let content_key = content_key(&fingerprint, *occurrence);
        let affinity_key = affinity_key(&node);
        *occurrence += 1;
        prepared.push(PreparedNodeIdentity {
            node,
            fingerprint,
            stable_key,
            content_key,
            affinity_key,
        });
    }
    prepared
}

/// Historical identity reservations remain intact for returning nodes and
/// dormant rules. Index them once; never scan the entire history per node.
struct BindingHistoryIndex {
    exact_index: HashMap<String, usize>,
    content_index: HashMap<String, Vec<usize>>,
    affinity_index: HashMap<String, Vec<usize>>,
    old_affinity_counts: HashMap<String, usize>,
}

impl BindingHistoryIndex {
    fn build(bindings: &[NodeTagBinding]) -> Self {
        let mut exact_index = HashMap::new();
        let mut content_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut affinity_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut old_affinity_counts: HashMap<String, usize> = HashMap::new();

        for (index, binding) in bindings.iter().enumerate() {
            *old_affinity_counts
                .entry(binding.affinity_key.clone())
                .or_default() += 1;
            exact_index
                .entry(binding.stable_key.clone())
                .or_insert(index);
            content_index
                .entry(binding.content_key.clone())
                .or_default()
                .push(index);
            affinity_index
                .entry(binding.affinity_key.clone())
                .or_default()
                .push(index);
        }

        Self {
            exact_index,
            content_index,
            affinity_index,
            old_affinity_counts,
        }
    }

    /// Match an incoming node against unassigned historical bindings in priority order:
    /// 1. Exact (`stable_key`: same source + outbound config + duplicate ordinal)
    /// 2. Content (`content_key`: same outbound config + ordinal across URL change)
    /// 3. Unique Affinity (`affinity_key`: 1-to-1 endpoint match across credential/name rotation)
    fn find_reusable_binding(
        &self,
        candidate: &PreparedNodeIdentity,
        current_affinity_counts: &HashMap<String, usize>,
        bindings: &[NodeTagBinding],
        assigned_tags: &HashSet<String>,
    ) -> Option<usize> {
        let available = |index: &usize| !assigned_tags.contains(&bindings[*index].tag);

        if let Some(exact) = self
            .exact_index
            .get(&candidate.stable_key)
            .copied()
            .filter(available)
        {
            return Some(exact);
        }

        if let Some(content_match) = self
            .content_index
            .get(&candidate.content_key)
            .and_then(|indexes| indexes.iter().copied().find(available))
        {
            return Some(content_match);
        }

        let unique_endpoint_in_both = current_affinity_counts.get(&candidate.affinity_key)
            == Some(&1)
            && self.old_affinity_counts.get(&candidate.affinity_key) == Some(&1);
        if unique_endpoint_in_both {
            return self
                .affinity_index
                .get(&candidate.affinity_key)
                .and_then(|indexes| indexes.iter().copied().find(available));
        }

        None
    }
}

fn claim_active_or_unique_tag(
    candidate: &PreparedNodeIdentity,
    manual_tags: &[String],
    assigned_tags: &HashSet<String>,
    active_tags: &mut HashMap<Vec<u8>, Vec<String>>,
    used: &mut HashSet<String>,
) -> String {
    let active = active_tags
        .get_mut(&candidate.fingerprint)
        .and_then(|tags| {
            tags.iter()
                .position(|tag| !manual_tags.contains(tag) && !assigned_tags.contains(tag))
                .map(|index| tags.remove(index))
        });
    let tag = active.unwrap_or_else(|| make_unique_tag(&candidate.node.name, used));
    used.insert(tag.clone());
    tag
}

/// Assign stable public tags while retaining the legacy display format.
pub async fn assign_subscription_tags(
    state: &Arc<AppState>,
    manual_tags: &[String],
    reserved_rule_tags: &[String],
    nodes: Vec<FetchedNode>,
) -> (Vec<String>, Vec<serde_json::Value>, NodeTagBindings) {
    let mut bindings = match load_bindings(state).await {
        Ok(Some(bindings)) => bindings,
        Ok(None) => NodeTagBindings::default(),
        Err(err) => {
            // Continue for compatibility, but custom-rule targets below remain
            // reserved so losing this auxiliary file cannot silently retarget a rule.
            warn!(error = %err, "Rebuilding unusable stable node tag bindings safely");
            NodeTagBindings::default()
        }
    };
    let mut used: HashSet<String> = manual_tags.iter().cloned().collect();
    used.insert("proxy".to_string());
    used.insert("direct".to_string());
    used.extend(reserved_rule_tags.iter().cloned());
    used.extend(bindings.bindings.iter().map(|binding| binding.tag.clone()));

    let prepared = prepare_node_identities(nodes);
    let current_affinity_counts: HashMap<String, usize> =
        prepared.iter().fold(HashMap::new(), |mut counts, item| {
            *counts.entry(item.affinity_key.clone()).or_default() += 1;
            counts
        });
    let history_index = BindingHistoryIndex::build(&bindings.bindings);
    let mut active_tags = active_tag_by_fingerprint(state).await;
    let mut assigned_tags = HashSet::new();
    let mut names = Vec::with_capacity(prepared.len());
    let mut outbounds = Vec::with_capacity(prepared.len());

    for candidate in prepared {
        let tag = if let Some(index) = history_index.find_reusable_binding(
            &candidate,
            &current_affinity_counts,
            &bindings.bindings,
            &assigned_tags,
        ) {
            let existing = &mut bindings.bindings[index];
            existing.stable_key = candidate.stable_key;
            existing.content_key = candidate.content_key;
            existing.affinity_key = candidate.affinity_key;
            existing.tag.clone()
        } else {
            let tag = claim_active_or_unique_tag(
                &candidate,
                manual_tags,
                &assigned_tags,
                &mut active_tags,
                &mut used,
            );
            bindings.bindings.push(NodeTagBinding {
                stable_key: candidate.stable_key,
                content_key: candidate.content_key,
                affinity_key: candidate.affinity_key,
                tag: tag.clone(),
            });
            tag
        };

        assigned_tags.insert(tag.clone());
        let mut outbound = candidate.node.outbound;
        if let Some(object) = outbound.as_object_mut() {
            object.insert("tag".to_string(), serde_json::Value::String(tag.clone()));
        }
        names.push(tag);
        outbounds.push(outbound);
    }

    (names, outbounds, bindings)
}

pub async fn save_node_bindings(state: &AppState, bindings: &NodeTagBindings) -> AppResult<()> {
    let bytes = serde_json::to_vec(bindings)?;
    if tokio::fs::read(&state.runtime_paths.node_bindings)
        .await
        .ok()
        .as_deref()
        == Some(bytes.as_slice())
    {
        return Ok(());
    }
    write_file_atomic(&state.runtime_paths.node_bindings, &bytes)
        .await
        .map_err(|e| AppError::context("Failed to persist stable node tag bindings", e))
}

#[cfg(test)]
mod tests {
    use super::{assign_subscription_tags, load_bindings, save_node_bindings, FetchedNode};
    use crate::{models::Config, test_support::app_state};

    fn node(name: &str, server: &str) -> FetchedNode {
        FetchedNode {
            source_id: "source-a".to_string(),
            name: name.to_string(),
            outbound: serde_json::json!({
                "type": "trojan",
                "tag": name,
                "server": server,
                "server_port": 443,
                "password": "secret"
            }),
        }
    }

    #[tokio::test]
    async fn duplicate_tags_stay_bound_when_subscription_order_changes() {
        let state = app_state(Config::default());
        let (_, first, bindings) = assign_subscription_tags(
            &state,
            &[],
            &[],
            vec![
                node("Hong Kong", "a.example.com"),
                node("Hong Kong", "b.example.com"),
            ],
        )
        .await;
        save_node_bindings(&state, &bindings).await.unwrap();
        assert_eq!(first[0]["tag"], "Hong Kong");
        assert_eq!(first[1]["tag"], "Hong Kong (2)");

        let (_, reordered, _) = assign_subscription_tags(
            &state,
            &[],
            &[],
            vec![
                node("Hong Kong", "b.example.com"),
                node("Hong Kong", "a.example.com"),
            ],
        )
        .await;
        assert_eq!(reordered[0]["tag"], "Hong Kong (2)");
        assert_eq!(reordered[1]["tag"], "Hong Kong");
    }

    #[tokio::test]
    async fn disappeared_tag_is_not_reused() {
        let state = app_state(Config::default());
        let (_, _, bindings) = assign_subscription_tags(
            &state,
            &[],
            &[],
            vec![node("same", "a.example.com"), node("same", "b.example.com")],
        )
        .await;
        save_node_bindings(&state, &bindings).await.unwrap();
        let (_, outbounds, _) = assign_subscription_tags(
            &state,
            &[],
            &[],
            vec![node("same", "a.example.com"), node("same", "c.example.com")],
        )
        .await;
        assert_eq!(outbounds[0]["tag"], "same");
        assert_eq!(outbounds[1]["tag"], "same (3)");
    }

    #[tokio::test]
    async fn unique_endpoint_keeps_tag_when_credentials_rotate() {
        let state = app_state(Config::default());
        let original = node("stable-name", "a.example.com");
        let (_, _, bindings) = assign_subscription_tags(&state, &[], &[], vec![original]).await;
        save_node_bindings(&state, &bindings).await.unwrap();

        let mut rotated = node("renamed-by-provider", "a.example.com");
        rotated.outbound["password"] = serde_json::json!("rotated-secret");
        let (_, outbounds, _) = assign_subscription_tags(&state, &[], &[], vec![rotated]).await;

        assert_eq!(outbounds[0]["tag"], "stable-name");
    }

    #[tokio::test]
    async fn invalid_bindings_fail_closed_for_dormant_rule_targets() {
        let state = app_state(Config::default());
        if let Some(parent) = state.runtime_paths.node_bindings.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&state.runtime_paths.node_bindings, b"not-json")
            .await
            .unwrap();
        assert!(load_bindings(&state).await.is_err());

        let reserved = vec!["disappeared-node".to_string()];
        let (_, outbounds, _) = assign_subscription_tags(
            &state,
            &[],
            &reserved,
            vec![node("disappeared-node", "new.example.com")],
        )
        .await;

        assert_eq!(outbounds[0]["tag"], "disappeared-node (2)");
        let _ = tokio::fs::remove_file(&state.runtime_paths.node_bindings).await;
    }
}
