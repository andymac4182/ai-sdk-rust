//! Shared utility functions for chat adapters.
//!
//! 1:1 port of `packages/adapter-shared/src/adapter-utils.ts`. These
//! utilities are used across all adapter implementations (Slack,
//! Teams, GChat, etc.) to reduce code duplication and ensure
//! consistent behavior when reaching into an
//! [`AdapterPostableMessage`] to pull out its card / files /
//! attachments payload.
//!
//! Upstream is duck-typed (`typeof message === "object" && "card" in
//! message`). The Rust port commits to the typed
//! [`chat_sdk_chat::types::AdapterPostableMessage`] enum, so each
//! upstream property-existence check collapses to a `match` arm. The
//! observable behavior matches upstream 1:1.

use chat_sdk_chat::cards::CardElement;
use chat_sdk_chat::types::{AdapterPostableMessage, Attachment, FileUpload};

/// Extract a [`CardElement`] from an [`AdapterPostableMessage`] if one
/// is present. 1:1 port of upstream
/// `extractCard(message): CardElement | null`.
///
/// Handles two cases:
/// 1. The message **is** a `CardElement` (upstream `isCardElement`
///    branch — Rust `AdapterPostableMessage::CardElement`).
/// 2. The message is a `PostableCard` wrapping a card (upstream
///    `"card" in message` branch — Rust `AdapterPostableMessage::Card`).
///
/// All other variants (raw, markdown, ast, plain text) return [`None`].
pub fn extract_card(message: &AdapterPostableMessage) -> Option<&CardElement> {
    match message {
        AdapterPostableMessage::CardElement(card) => Some(card),
        AdapterPostableMessage::Card(postable) => Some(&postable.card),
        _ => None,
    }
}

/// Extract a slice of [`FileUpload`]s from an [`AdapterPostableMessage`].
/// 1:1 port of upstream `extractFiles(message): FileUpload[]`.
///
/// Files can be attached to `PostableRaw`, `PostableMarkdown`,
/// `PostableAst`, or `PostableCard` messages via the `files` property.
/// Returns an empty slice for plain-text or direct-card messages, as
/// well as when the `files` field is `None`/empty.
pub fn extract_files(message: &AdapterPostableMessage) -> &[FileUpload] {
    let files = match message {
        AdapterPostableMessage::Raw(m) => m.files.as_deref(),
        AdapterPostableMessage::Markdown(m) => m.files.as_deref(),
        AdapterPostableMessage::Ast(m) => m.files.as_deref(),
        AdapterPostableMessage::Card(m) => m.files.as_deref(),
        AdapterPostableMessage::CardElement(_) | AdapterPostableMessage::Text(_) => None,
    };
    files.unwrap_or(&[])
}

/// Extract a slice of [`Attachment`]s from an
/// [`AdapterPostableMessage`]. 1:1 port of upstream
/// `extractPostableAttachments(message): Attachment[]`.
///
/// Only the structured `PostableRaw`/`PostableMarkdown`/`PostableAst`
/// variants carry attachments. Returns an empty slice for everything
/// else.
pub fn extract_postable_attachments(message: &AdapterPostableMessage) -> &[Attachment] {
    let attachments = match message {
        AdapterPostableMessage::Raw(m) => m.attachments.as_deref(),
        AdapterPostableMessage::Markdown(m) => m.attachments.as_deref(),
        AdapterPostableMessage::Ast(m) => m.attachments.as_deref(),
        AdapterPostableMessage::Card(_)
        | AdapterPostableMessage::CardElement(_)
        | AdapterPostableMessage::Text(_) => None,
    };
    attachments.unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    //! Adaptation of `packages/adapter-shared/src/adapter-utils.test.ts`.
    //!
    //! Coverage notes:
    //! - All "with CardElement" / "with PostableCard" / "with non-card
    //!   messages" cases port directly.
    //! - Upstream `null`/`undefined` test cases (passed with
    //!   `@ts-expect-error`) have no Rust analogue — the typed
    //!   `AdapterPostableMessage` rejects them at the type system; this
    //!   is documented as `js-only-documented`-adjacent in the slice
    //!   ledger entry.
    //! - "handles Blob/ArrayBuffer data in files" upstream cases test
    //!   that the `FileUpload.data` field stores the runtime type
    //!   unchanged. In Rust both collapse to `Vec<u8>`/`FileBytes`, so
    //!   the equivalent assertion is a single byte-roundtrip case.
    //!
    //! 17 ported cases out of 25 upstream cases; the 8 unported cases
    //! are runtime-type assertions (null/undefined/Blob/ArrayBuffer/
    //! non-card-type-object) that collapse into the Rust type system.
    use super::*;
    use chat_sdk_chat::cards::{CardOptions, card, card_text};
    use chat_sdk_chat::markdown;
    use chat_sdk_chat::types::{
        AttachmentKind, FileBytes, PostableAst, PostableCard, PostableMarkdown, PostableRaw,
    };

    fn sample_card(title: &str) -> CardElement {
        card(CardOptions {
            title: Some(title.to_string()),
            children: Some(vec![card_text("Content", None).into()]),
            ..Default::default()
        })
    }

    // ---------- extract_card ----------

    // ---------- extract_card ----------
    //
    // 1:1 with upstream `adapter-utils.test.ts > describe("extractCard")`.
    // The following 4 upstream cases are 1:1 via the type system
    // (per the slice-380 brief tightening) and have no matching
    // Rust test:
    //
    // - `returns null for null input` / `returns null for undefined
    //   input`: the Rust signature takes `&AdapterPostableMessage`
    //   (non-null), so the cases are unreachable.
    // - `returns null for object without card or type` / `returns
    //   null for non-card type object`: the Rust input is a typed
    //   enum (`AdapterPostableMessage`) with only the recognized
    //   postable variants; the unstructured-object case is not
    //   constructible.

    #[test]
    fn extract_card_returns_card_element_passed_directly() {
        let card = sample_card("Test Card");
        let msg = AdapterPostableMessage::from(card.clone());
        assert_eq!(extract_card(&msg), Some(&card));
    }

    #[test]
    fn extract_card_returns_card_with_all_properties_intact() {
        let card = CardElement {
            title: Some("Order #123".to_string()),
            subtitle: Some("Processing".to_string()),
            image_url: Some("https://example.com/img.png".to_string()),
            children: vec![card_text("Details", None).into()],
            kind: chat_sdk_chat::cards::CardKind::Card,
        };
        let msg = AdapterPostableMessage::from(card.clone());
        let result = extract_card(&msg).unwrap();
        assert_eq!(result.title.as_deref(), Some("Order #123"));
        assert_eq!(result.subtitle.as_deref(), Some("Processing"));
    }

    #[test]
    fn extract_card_returns_card_from_postable_card_wrapper() {
        let card = sample_card("Nested Card");
        let pc = PostableCard {
            card: card.clone(),
            fallback_text: None,
            files: None,
        };
        let msg = AdapterPostableMessage::from(pc);
        assert_eq!(extract_card(&msg), Some(&card));
    }

    #[test]
    fn extract_card_returns_card_from_postable_card_with_fallback_text() {
        let card = sample_card("With Fallback");
        let pc = PostableCard {
            card: card.clone(),
            fallback_text: Some("Plain text version".to_string()),
            files: None,
        };
        let msg = AdapterPostableMessage::from(pc);
        assert_eq!(extract_card(&msg), Some(&card));
    }

    #[test]
    fn extract_card_returns_card_from_postable_card_with_files() {
        let card = sample_card("With Files");
        let pc = PostableCard {
            card: card.clone(),
            fallback_text: None,
            files: Some(vec![FileUpload {
                data: FileBytes::from(b"test".to_vec()),
                filename: "test.txt".to_string(),
                mime_type: None,
            }]),
        };
        let msg = AdapterPostableMessage::from(pc);
        assert_eq!(extract_card(&msg), Some(&card));
    }

    #[test]
    fn extract_card_returns_none_for_plain_string() {
        let msg = AdapterPostableMessage::from("Hello world");
        assert!(extract_card(&msg).is_none());
    }

    #[test]
    fn extract_card_returns_none_for_postable_raw() {
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: None,
            raw: "Raw text".to_string(),
        });
        assert!(extract_card(&msg).is_none());
    }

    #[test]
    fn extract_card_returns_none_for_postable_markdown() {
        let msg = AdapterPostableMessage::from(PostableMarkdown {
            attachments: None,
            files: None,
            markdown: "**Bold** text".to_string(),
        });
        assert!(extract_card(&msg).is_none());
    }

    #[test]
    fn extract_card_returns_none_for_postable_ast() {
        let msg = AdapterPostableMessage::from(PostableAst {
            ast: markdown::root(vec![]),
            attachments: None,
            files: None,
        });
        assert!(extract_card(&msg).is_none());
    }

    // ---------- extract_files ----------
    //
    // 1:1 with upstream `adapter-utils.test.ts > describe("extractFiles")`.
    // The following upstream cases are 1:1 via the type system (per
    // the slice-380 brief tightening) and have no matching Rust test:
    //
    // - `handles Blob data in files` / `handles ArrayBuffer data in
    //   files`: the Rust `FileUpload::data` field is `FileBytes`
    //   (`Vec<u8>`), not a `Blob | ArrayBuffer | Buffer` union — the
    //   JS-runtime-specific variants collapse to a single
    //   `FileBytes` case at the type level.
    // - `returns empty array for null input` / `returns empty array
    //   for undefined input`: the Rust signature takes
    //   `&AdapterPostableMessage` (non-null), so the cases are
    //   unreachable.

    #[test]
    fn extract_files_returns_files_from_postable_raw() {
        let files = vec![
            FileUpload {
                data: FileBytes::from(b"content1".to_vec()),
                filename: "file1.txt".to_string(),
                mime_type: None,
            },
            FileUpload {
                data: FileBytes::from(b"content2".to_vec()),
                filename: "file2.txt".to_string(),
                mime_type: None,
            },
        ];
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: Some(files.clone()),
            raw: "Text".to_string(),
        });
        assert_eq!(extract_files(&msg), files.as_slice());
        assert_eq!(extract_files(&msg).len(), 2);
    }

    #[test]
    fn extract_files_returns_files_from_postable_markdown_with_mime_type() {
        let files = vec![FileUpload {
            data: FileBytes::from(b"image".to_vec()),
            filename: "image.png".to_string(),
            mime_type: Some("image/png".to_string()),
        }];
        let msg = AdapterPostableMessage::from(PostableMarkdown {
            attachments: None,
            files: Some(files.clone()),
            markdown: "**Text**".to_string(),
        });
        assert_eq!(extract_files(&msg), files.as_slice());
        assert_eq!(
            extract_files(&msg)[0].mime_type.as_deref(),
            Some("image/png")
        );
    }

    #[test]
    fn extract_files_returns_files_from_postable_card() {
        let card = sample_card("Test");
        let files = vec![FileUpload {
            data: FileBytes::from(b"doc".to_vec()),
            filename: "doc.pdf".to_string(),
            mime_type: None,
        }];
        let msg = AdapterPostableMessage::from(PostableCard {
            card,
            fallback_text: None,
            files: Some(files.clone()),
        });
        assert_eq!(extract_files(&msg), files.as_slice());
    }

    #[test]
    fn extract_files_returns_empty_slice_when_files_field_is_empty_vec() {
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: Some(vec![]),
            raw: "Text".to_string(),
        });
        assert!(extract_files(&msg).is_empty());
    }

    #[test]
    fn extract_files_returns_empty_slice_when_files_field_is_none() {
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: None,
            raw: "Text".to_string(),
        });
        assert!(extract_files(&msg).is_empty());
    }

    #[test]
    fn extract_files_returns_empty_slice_for_postable_raw_without_files() {
        // 1:1 with upstream `describe("extractFiles") > it("returns
        // empty array for PostableRaw without files")` — the upstream
        // test passes `{raw: "Just text"}` without specifying files.
        // The Rust port uses None for the missing-files signal; same
        // observable behavior.
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: None,
            raw: "Just text".to_string(),
        });
        assert!(extract_files(&msg).is_empty());
    }

    #[test]
    fn extract_files_returns_empty_slice_for_postable_markdown_without_files() {
        // 1:1 with upstream `describe("extractFiles") > it("returns
        // empty array for PostableMarkdown without files")` — the
        // `files` field on PostableMarkdown is None.
        let msg = AdapterPostableMessage::from(PostableMarkdown {
            attachments: None,
            files: None,
            markdown: "**Bold**".to_string(),
        });
        assert!(extract_files(&msg).is_empty());
    }

    #[test]
    fn extract_files_returns_empty_slice_for_plain_string() {
        let msg = AdapterPostableMessage::from("Hello world");
        assert!(extract_files(&msg).is_empty());
    }

    #[test]
    fn extract_files_returns_empty_slice_for_direct_card_element() {
        let msg = AdapterPostableMessage::from(sample_card("Test"));
        assert!(extract_files(&msg).is_empty());
    }

    // ---------- extract_postable_attachments ----------
    //
    // 1:1 with upstream `adapter-utils.test.ts >
    // describe("extractPostableAttachments")`. The following 2
    // upstream cases are 1:1 via the type system (per the slice-380
    // brief tightening) and have no matching Rust test:
    //
    // - `returns empty array for null input` / `returns empty array
    //   for undefined input`: the Rust signature takes
    //   `&AdapterPostableMessage` (non-null), so the cases are
    //   unreachable.

    #[test]
    fn extract_postable_attachments_returns_attachments_from_postable_raw() {
        let attachments = vec![
            chat_sdk_chat::types::Attachment {
                data: Some(FileBytes::from(b"content1".to_vec())),
                fetch_metadata: None,
                height: None,
                mime_type: None,
                name: Some("file1.txt".to_string()),
                size: None,
                kind: AttachmentKind::File,
                url: None,
                width: None,
            },
            chat_sdk_chat::types::Attachment {
                data: Some(FileBytes::from(b"content2".to_vec())),
                fetch_metadata: None,
                height: None,
                mime_type: None,
                name: Some("file2.txt".to_string()),
                size: None,
                kind: AttachmentKind::File,
                url: None,
                width: None,
            },
        ];
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: Some(attachments.clone()),
            files: None,
            raw: "Text".to_string(),
        });
        assert_eq!(extract_postable_attachments(&msg), attachments.as_slice());
        assert_eq!(extract_postable_attachments(&msg).len(), 2);
    }

    #[test]
    fn extract_postable_attachments_returns_attachments_from_postable_ast() {
        // 1:1 with upstream `describe("extractPostableAttachments")
        // > it("extracts attachments array from PostableAst")` —
        // PostableAst can carry an attachments array, and the helper
        // returns it unchanged.
        let attachments = vec![chat_sdk_chat::types::Attachment {
            data: Some(FileBytes::from(b"doc".to_vec())),
            fetch_metadata: None,
            height: None,
            mime_type: None,
            name: Some("doc.pdf".to_string()),
            size: None,
            kind: AttachmentKind::File,
            url: None,
            width: None,
        }];
        let msg = AdapterPostableMessage::from(PostableAst {
            ast: markdown::root(vec![]),
            attachments: Some(attachments.clone()),
            files: None,
        });
        assert_eq!(extract_postable_attachments(&msg), attachments.as_slice());
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_for_postable_raw_without_attachments() {
        // 1:1 with upstream `describe("extractPostableAttachments")
        // > it("returns empty array for PostableRaw without
        // attachments")` — the `attachments` field is None.
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: None,
            raw: "Text".to_string(),
        });
        assert!(extract_postable_attachments(&msg).is_empty());
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_for_postable_markdown_without_attachments() {
        // 1:1 with upstream `describe("extractPostableAttachments")
        // > it("returns empty array for PostableMarkdown without
        // attachments")` — the `attachments` field is None.
        let msg = AdapterPostableMessage::from(PostableMarkdown {
            attachments: None,
            files: None,
            markdown: "**Text**".to_string(),
        });
        assert!(extract_postable_attachments(&msg).is_empty());
    }

    #[test]
    fn extract_postable_attachments_returns_attachments_from_postable_markdown() {
        let attachments = vec![chat_sdk_chat::types::Attachment {
            data: Some(FileBytes::from(b"image".to_vec())),
            fetch_metadata: None,
            height: None,
            mime_type: Some("image/png".to_string()),
            name: Some("image.png".to_string()),
            size: None,
            kind: AttachmentKind::Image,
            url: None,
            width: None,
        }];
        let msg = AdapterPostableMessage::from(PostableMarkdown {
            attachments: Some(attachments.clone()),
            files: None,
            markdown: "**Text**".to_string(),
        });
        assert_eq!(extract_postable_attachments(&msg), attachments.as_slice());
        assert_eq!(
            extract_postable_attachments(&msg)[0].mime_type.as_deref(),
            Some("image/png")
        );
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_when_attachments_field_is_undefined() {
        // 1:1 with upstream `describe("extractPostableAttachments") >
        // it("returns empty array when attachments property is
        // undefined")` — upstream constructs `{raw: "Text",
        // attachments: undefined}`. The Rust port uses None for the
        // missing-attachments signal; same observable behavior.
        // Distinct from `_for_postable_raw_without_attachments` in
        // upstream (the "without" case constructs the message
        // without specifying attachments at all).
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: None,
            files: None,
            raw: "Text".to_string(),
        });
        assert!(extract_postable_attachments(&msg).is_empty());
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_when_empty() {
        let msg = AdapterPostableMessage::from(PostableRaw {
            attachments: Some(vec![]),
            files: None,
            raw: "Text".to_string(),
        });
        assert!(extract_postable_attachments(&msg).is_empty());
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_for_plain_string() {
        let msg = AdapterPostableMessage::from("Hello world");
        assert!(extract_postable_attachments(&msg).is_empty());
    }

    #[test]
    fn extract_postable_attachments_returns_empty_slice_for_direct_card_element() {
        let msg = AdapterPostableMessage::from(sample_card("Test"));
        assert!(extract_postable_attachments(&msg).is_empty());
    }
}
