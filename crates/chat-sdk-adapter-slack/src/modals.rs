//! Slack modal-metadata codec helpers.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/modals.ts`.
//! This slice covers the `encodeModalMetadata` /
//! `decodeModalMetadata` pair + the `ModalMetadata` shape. The
//! `modalToSlackView` + `selectOptionToSlackOption` conversion
//! helpers depend on Slack's view JSON structure and follow in
//! later slices.

/// Decoded modal metadata. 1:1 with upstream
/// `interface ModalMetadata`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModalMetadata {
    /// Per-modal context id used by chat-sdk to correlate a
    /// `views.open` with subsequent `view_submission` /
    /// `view_closed` events.
    pub context_id: Option<String>,
    /// Caller-supplied opaque metadata. Round-trips through
    /// Slack's `private_metadata` field.
    pub private_metadata: Option<String>,
}

/// Encode contextId + privateMetadata into Slack's
/// `private_metadata` field. 1:1 with upstream
/// `encodeModalMetadata(meta)`:
///
/// - Returns `None` when both fields are absent.
/// - Otherwise returns `JSON.stringify({c, m})` where each key
///   is omitted when its value is None.
pub fn encode_modal_metadata(meta: &ModalMetadata) -> Option<String> {
    if meta.context_id.is_none() && meta.private_metadata.is_none() {
        return None;
    }
    let mut obj = serde_json::Map::with_capacity(2);
    if let Some(c) = &meta.context_id {
        obj.insert("c".to_string(), serde_json::Value::String(c.clone()));
    }
    if let Some(m) = &meta.private_metadata {
        obj.insert("m".to_string(), serde_json::Value::String(m.clone()));
    }
    Some(serde_json::Value::Object(obj).to_string())
}

/// Decode Slack's `private_metadata` back into a [`ModalMetadata`].
/// 1:1 with upstream `decodeModalMetadata(raw?)`:
///
/// - `None` / empty -> `ModalMetadata::default()`.
/// - Well-formed `{c, m}` JSON -> decoded fields. Empty-string
///   values fall back to `None`.
/// - Anything else (legacy plain-string, JSON missing both keys,
///   malformed JSON) -> `{context_id: Some(raw), private_metadata:
///   None}` so legacy callers that stored a raw UUID still work.
pub fn decode_modal_metadata(raw: Option<&str>) -> ModalMetadata {
    let Some(raw) = raw.filter(|s| !s.is_empty()) else {
        return ModalMetadata::default();
    };
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw)
        && parsed.is_object()
    {
        let has_c = parsed.get("c").is_some();
        let has_m = parsed.get("m").is_some();
        if has_c || has_m {
            let context_id = parsed
                .get("c")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let private_metadata = parsed
                .get("m")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            return ModalMetadata {
                context_id,
                private_metadata,
            };
        }
    }
    // Legacy passthrough: treat the raw string as a plain
    // contextId.
    ModalMetadata {
        context_id: Some(raw.to_string()),
        private_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- encodeModalMetadata (4 upstream cases) ----------

    #[test]
    fn returns_none_when_both_fields_are_empty() {
        assert!(encode_modal_metadata(&ModalMetadata::default()).is_none());
    }

    #[test]
    fn encodes_context_id_only() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: Some("uuid-123".to_string()),
            private_metadata: None,
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["c"], "uuid-123");
        assert!(parsed.get("m").is_none());
    }

    #[test]
    fn encodes_private_metadata_only() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: None,
            private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert!(parsed.get("c").is_none());
        assert_eq!(parsed["m"], r#"{"chatId":"abc"}"#);
    }

    #[test]
    fn encodes_both_context_id_and_private_metadata() {
        let encoded = encode_modal_metadata(&ModalMetadata {
            context_id: Some("uuid-123".to_string()),
            private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
        })
        .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(parsed["c"], "uuid-123");
        assert_eq!(parsed["m"], r#"{"chatId":"abc"}"#);
    }

    // ---------- decodeModalMetadata (8 upstream cases) ----------

    #[test]
    fn returns_empty_object_for_undefined_input() {
        assert_eq!(decode_modal_metadata(None), ModalMetadata::default());
    }

    #[test]
    fn returns_empty_object_for_empty_string() {
        assert_eq!(decode_modal_metadata(Some("")), ModalMetadata::default());
    }

    #[test]
    fn decodes_context_id_only() {
        let encoded = r#"{"c":"uuid-123"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: Some("uuid-123".to_string()),
                private_metadata: None,
            }
        );
    }

    #[test]
    fn decodes_private_metadata_only() {
        let encoded = r#"{"m":"{\"chatId\":\"abc\"}"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: None,
                private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
            }
        );
    }

    #[test]
    fn decodes_both_context_id_and_private_metadata() {
        let encoded = r#"{"c":"uuid-123","m":"{\"chatId\":\"abc\"}"}"#;
        assert_eq!(
            decode_modal_metadata(Some(encoded)),
            ModalMetadata {
                context_id: Some("uuid-123".to_string()),
                private_metadata: Some(r#"{"chatId":"abc"}"#.to_string()),
            }
        );
    }

    #[test]
    fn falls_back_to_treating_plain_string_as_context_id() {
        assert_eq!(
            decode_modal_metadata(Some("plain-uuid-456")),
            ModalMetadata {
                context_id: Some("plain-uuid-456".to_string()),
                private_metadata: None,
            }
        );
    }

    #[test]
    fn falls_back_for_json_without_c_or_m_keys() {
        let raw = r#"{"other":"value"}"#;
        assert_eq!(
            decode_modal_metadata(Some(raw)),
            ModalMetadata {
                context_id: Some(raw.to_string()),
                private_metadata: None,
            }
        );
    }

    // ---------- roundtrip (1 upstream case) ----------

    #[test]
    fn roundtrips_encode_then_decode() {
        let original = ModalMetadata {
            context_id: Some("ctx-1".to_string()),
            private_metadata: Some(r#"{"key":"val"}"#.to_string()),
        };
        let encoded = encode_modal_metadata(&original).unwrap();
        let decoded = decode_modal_metadata(Some(&encoded));
        assert_eq!(decoded, original);
    }
}
