//! Slot assignment for streamed Chat Completions `tool_calls` deltas.
//!
//! OpenAI-compliant upstreams tag every streamed tool-call fragment with an
//! `index`. Some OpenAI-compatible gateways and local servers omit it. This
//! resolver maps each fragment to a stable slot key so the streaming
//! converters neither drop index-less fragments nor collapse distinct
//! parallel calls into one:
//!
//! - `index` present: the index is the key (standard behaviour).
//! - `index` absent, `id` present: the slot already bound to that id, or a new
//!   slot when the id has not been seen.
//! - `index` absent, no `id`, but a `name` while the most recent slot already
//!   has a name: a new slot (a new call that omitted its id).
//! - otherwise: the most recently used slot (argument continuation).
//!
//! New slots take the next key after the highest key seen so far, so keys
//! stay dense and ordered for consumers that release calls in key order.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub(crate) struct ChatToolCallSlots {
    by_id: HashMap<String, usize>,
    named: HashSet<usize>,
    next_key: usize,
    last_key: Option<usize>,
}

impl ChatToolCallSlots {
    pub(crate) fn resolve(
        &mut self,
        index: Option<usize>,
        id: Option<&str>,
        name: Option<&str>,
    ) -> usize {
        let id = id.filter(|value| !value.is_empty());
        let name = name.filter(|value| !value.is_empty());

        let key = match index {
            Some(index) => index,
            None => match id {
                Some(id) => self.by_id.get(id).copied().unwrap_or(self.next_key),
                None => match self.last_key {
                    Some(last) if name.is_some() && self.named.contains(&last) => self.next_key,
                    Some(last) => last,
                    None => self.next_key,
                },
            },
        };

        if let Some(id) = id {
            self.by_id.entry(id.to_string()).or_insert(key);
        }
        if name.is_some() {
            self.named.insert(key);
        }
        self.next_key = self.next_key.max(key.saturating_add(1));
        self.last_key = Some(key);
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_index_is_used_verbatim() {
        let mut slots = ChatToolCallSlots::default();
        assert_eq!(slots.resolve(Some(0), Some("a"), Some("f")), 0);
        assert_eq!(slots.resolve(Some(1), Some("b"), Some("g")), 1);
        assert_eq!(slots.resolve(Some(0), None, None), 0);
    }

    #[test]
    fn missing_index_with_distinct_ids_opens_distinct_slots() {
        let mut slots = ChatToolCallSlots::default();
        assert_eq!(slots.resolve(None, Some("call_a"), Some("f")), 0);
        assert_eq!(slots.resolve(None, None, None), 0);
        assert_eq!(slots.resolve(None, Some("call_b"), Some("g")), 1);
        assert_eq!(slots.resolve(None, None, None), 1);
        assert_eq!(slots.resolve(None, Some("call_a"), None), 0);
    }

    #[test]
    fn missing_index_and_id_with_new_name_opens_new_slot() {
        let mut slots = ChatToolCallSlots::default();
        assert_eq!(slots.resolve(None, None, Some("f")), 0);
        assert_eq!(slots.resolve(None, None, None), 0);
        assert_eq!(slots.resolve(None, None, Some("g")), 1);
    }

    #[test]
    fn index_less_slots_follow_highest_explicit_index() {
        let mut slots = ChatToolCallSlots::default();
        assert_eq!(slots.resolve(Some(2), Some("a"), Some("f")), 2);
        assert_eq!(slots.resolve(None, Some("b"), Some("g")), 3);
    }
}
