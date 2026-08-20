use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

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
}

impl Default for NodeTagBindings {
    fn default() -> Self {
        Self {
            version: BINDINGS_VERSION,
            bindings: Vec::new(),
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

fn stable_key(node: &FetchedNode, duplicate_index: usize) -> String {
    let bytes = outbound_bytes(&node.outbound);
    sha256_parts(&[
        node.source_id.as_bytes(),
        &bytes,
        duplicate_index.to_string().as_bytes(),
    ])
}

fn content_key(node: &FetchedNode, duplicate_index: usize) -> String {
    let bytes = outbound_bytes(&node.outbound);
    sha256_parts(&[&bytes, duplicate_index.to_string().as_bytes()])
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

fn make_unique_tag(base: &str, used: &mut HashSet<String>) -> String {
    let base = if base.trim().is_empty() { "node" } else { base };
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base} ({index})");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("unbounded duplicate tag search should find a value")
}

async fn load_bindings(state: &AppState) -> Option<NodeTagBindings> {
    let bytes = tokio::fs::read(&state.runtime_paths.node_bindings)
        .await
        .ok()?;
    let parsed: NodeTagBindings = serde_json::from_slice(&bytes).ok()?;
    (parsed.version == BINDINGS_VERSION).then_some(parsed)
}

pub async fn reserved_node_tags(state: &AppState) -> Vec<String> {
    load_bindings(state)
        .await
        .map(|bindings| {
            bindings
                .bindings
                .into_iter()
                .map(|binding| binding.tag)
                .collect()
        })
        .unwrap_or_default()
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

/// Assign stable public tags while retaining the legacy display format.
pub async fn assign_subscription_tags(
    state: &Arc<AppState>,
    manual_tags: &[String],
    nodes: Vec<FetchedNode>,
) -> (Vec<String>, Vec<serde_json::Value>, NodeTagBindings) {
    let mut bindings = load_bindings(state).await.unwrap_or_default();
    let mut used: HashSet<String> = manual_tags.iter().cloned().collect();
    used.insert("proxy".to_string());
    used.insert("direct".to_string());
    used.extend(bindings.bindings.iter().map(|binding| binding.tag.clone()));

    let mut fingerprint_occurrences: HashMap<(String, Vec<u8>), usize> = HashMap::new();
    let mut prepared = Vec::with_capacity(nodes.len());
    for node in nodes {
        let fingerprint = outbound_bytes(&node.outbound);
        let occurrence = fingerprint_occurrences
            .entry((node.source_id.clone(), fingerprint))
            .or_default();
        let key = stable_key(&node, *occurrence);
        let content = content_key(&node, *occurrence);
        *occurrence += 1;
        prepared.push((node, key, content));
    }

    let affinity_counts: HashMap<String, usize> =
        prepared
            .iter()
            .fold(HashMap::new(), |mut counts, (node, _, _)| {
                *counts.entry(affinity_key(node)).or_default() += 1;
                counts
            });
    let mut old_affinity_counts = HashMap::new();
    for binding in &bindings.bindings {
        *old_affinity_counts
            .entry(binding.affinity_key.clone())
            .or_insert(0usize) += 1;
    }
    let mut active_tags = active_tag_by_fingerprint(state).await;
    let mut assigned_tags = HashSet::new();
    let mut names = Vec::with_capacity(prepared.len());
    let mut outbounds = Vec::with_capacity(prepared.len());

    for (node, key, content) in prepared {
        let affinity = affinity_key(&node);
        let exact = bindings
            .bindings
            .iter()
            .position(|binding| binding.stable_key == key);
        let content_match = exact.or_else(|| {
            bindings.bindings.iter().position(|binding| {
                binding.content_key == content && !assigned_tags.contains(&binding.tag)
            })
        });
        let transferable = if content_match.is_none()
            && affinity_counts.get(&affinity) == Some(&1)
            && old_affinity_counts.get(&affinity) == Some(&1)
        {
            bindings.bindings.iter().position(|binding| {
                binding.affinity_key == affinity && !assigned_tags.contains(&binding.tag)
            })
        } else {
            None
        };

        let binding_index = content_match.or(transferable);
        let tag = if let Some(index) = binding_index {
            bindings.bindings[index].stable_key = key.clone();
            bindings.bindings[index].content_key = content.clone();
            bindings.bindings[index].affinity_key = affinity.clone();
            bindings.bindings[index].tag.clone()
        } else {
            let fingerprint = outbound_bytes(&node.outbound);
            let active = active_tags.get_mut(&fingerprint).and_then(|tags| {
                tags.iter()
                    .position(|tag| !manual_tags.contains(tag) && !assigned_tags.contains(tag))
                    .map(|index| tags.remove(index))
            });
            let tag = active.unwrap_or_else(|| make_unique_tag(&node.name, &mut used));
            used.insert(tag.clone());
            bindings.bindings.push(NodeTagBinding {
                stable_key: key,
                content_key: content,
                affinity_key: affinity,
                tag: tag.clone(),
            });
            tag
        };

        assigned_tags.insert(tag.clone());
        let mut outbound = node.outbound;
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
    use super::{assign_subscription_tags, save_node_bindings, FetchedNode};
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
            vec![node("same", "a.example.com"), node("same", "b.example.com")],
        )
        .await;
        save_node_bindings(&state, &bindings).await.unwrap();
        let (_, outbounds, _) = assign_subscription_tags(
            &state,
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
        let (_, _, bindings) = assign_subscription_tags(&state, &[], vec![original]).await;
        save_node_bindings(&state, &bindings).await.unwrap();

        let mut rotated = node("renamed-by-provider", "a.example.com");
        rotated.outbound["password"] = serde_json::json!("rotated-secret");
        let (_, outbounds, _) = assign_subscription_tags(&state, &[], vec![rotated]).await;

        assert_eq!(outbounds[0]["tag"], "stable-name");
    }
}
