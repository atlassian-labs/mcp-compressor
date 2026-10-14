//! Session-local, bounded recovery cache. Never memoizes ordinary tool calls.
use rmcp::model::CallToolResult;
use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 32;
const TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(super) struct ResponseCache {
    entries: VecDeque<Entry>,
}
impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseCache")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}
struct Entry {
    key: String,
    value: CallToolResult,
    bytes: usize,
    created: Instant,
}

pub(super) fn key(backend: &str, tool: &str, arguments: &Value, context: &Value) -> String {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    serde_json::json!([backend, tool, canonical(arguments), canonical(context)]).to_string()
}
impl ResponseCache {
    pub fn get(&mut self, key: &str) -> Option<CallToolResult> {
        self.entries.retain(|entry| entry.created.elapsed() < TTL);
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.key == key)
            .map(|entry| entry.value.clone())
    }
    pub fn insert(&mut self, key: String, value: CallToolResult) {
        let bytes = serde_json::to_vec(&value).map_or(MAX_BYTES + 1, |v| v.len()) + key.len();
        self.entries
            .retain(|entry| entry.key != key && entry.created.elapsed() < TTL);
        if bytes > MAX_BYTES {
            return;
        }
        while self.entries.len() >= MAX_ENTRIES
            || self.entries.iter().map(|e| e.bytes).sum::<usize>() + bytes > MAX_BYTES
        {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            key,
            value,
            bytes,
            created: Instant::now(),
        });
    }
    pub fn remove(&mut self, key: &str) {
        self.entries.retain(|entry| entry.key != key);
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock;
    use serde_json::json;
    #[test]
    fn keys_include_backend_tool_arguments_and_context_but_ignore_object_order() {
        let a = key("a", "read", &json!({"b":2,"a":1}), &json!({"tenant":"one"}));
        assert_eq!(
            a,
            key("a", "read", &json!({"a":1,"b":2}), &json!({"tenant":"one"}))
        );
        for b in [
            key("b", "read", &json!({"a":1,"b":2}), &json!({"tenant":"one"})),
            key(
                "a",
                "write",
                &json!({"a":1,"b":2}),
                &json!({"tenant":"one"}),
            ),
            key("a", "read", &json!({"a":2,"b":2}), &json!({"tenant":"one"})),
            key("a", "read", &json!({"a":1,"b":2}), &json!({"tenant":"two"})),
        ] {
            assert_ne!(a, b);
        }
    }
    #[test]
    fn byte_limit_rejects_oversized_entries_and_debug_redacts_data() {
        let mut cache = ResponseCache::default();
        cache.insert(
            "sensitive-key".into(),
            CallToolResult::success(vec![ContentBlock::text("private-data")]),
        );
        let debug = format!("{cache:?}");
        assert!(!debug.contains("private-data") && !debug.contains("sensitive-key"));
        cache.insert(
            "huge".into(),
            CallToolResult::success(vec![ContentBlock::text("x".repeat(MAX_BYTES))]),
        );
        assert!(cache.get("huge").is_none());
        assert!(cache.get("sensitive-key").is_some());
    }

    #[test]
    fn evicts_expires_and_clears() {
        let mut cache = ResponseCache::default();
        for n in 0..=MAX_ENTRIES {
            cache.insert(
                n.to_string(),
                CallToolResult::success(vec![ContentBlock::text(n.to_string())]),
            );
        }
        assert!(cache.get("0").is_none());
        assert!(cache.get("1").is_some());
        cache.entries.front_mut().unwrap().created = Instant::now() - TTL;
        assert!(cache.get("1").is_none());
        cache.clear();
        assert!(cache.entries.is_empty());
    }
}
