use serde_json::json;

use super::*;

fn sha(byte: char) -> String {
    format!("0x{}", byte.to_string().repeat(64))
}

fn entry(path: &str) -> ContentLedgerEntry {
    ContentLedgerEntry::new(
        format!("route:/{path}"),
        format!("/{path}"),
        path.to_string(),
        ContentLedgerCategory::for_path(path),
        vec![ContentFile {
            path: format!("content/{path}"),
            sha256: sha('a'),
            bytes: 3,
        }],
    )
    .unwrap()
}

fn input(date: Option<&str>, path: &str) -> ContentLedgerInput {
    ContentLedgerInput::new(
        ContentLedgerSortKey::new(date.map(str::to_string), path.to_string()),
        entry(path),
    )
}

#[test]
fn ledger_hash_validates_without_metadata_fields() {
    let ledger = ContentLedger::new(vec![input(Some("2026-04-01"), "writing/hello.md")]).unwrap();
    ledger.validate().unwrap();
    let encoded = serde_json::to_string(&ledger).unwrap();
    assert!(!encoded.contains("title"));
    assert!(!encoded.contains("description"));
    assert!(!encoded.contains("tags"));
    assert!(!encoded.contains("access"));
}

#[test]
fn ledger_validation_accepts_sidecar_in_primary_entry() {
    let entry = ContentLedgerEntry::new(
        "route:/talks/a.pdf".to_string(),
        "/talks/a.pdf".to_string(),
        "talks/a.pdf".to_string(),
        ContentLedgerCategory::Talks,
        vec![
            ContentFile {
                path: "content/talks/a.meta.json".to_string(),
                sha256: sha('b'),
                bytes: 19,
            },
            ContentFile {
                path: "content/talks/a.pdf".to_string(),
                sha256: sha('c'),
                bytes: 3,
            },
        ],
    )
    .unwrap();
    ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(Some("2026-04-01".to_string()), "talks/a.pdf".to_string()),
        entry,
    )])
    .unwrap()
    .validate()
    .unwrap();
}

#[test]
fn ledger_validation_accepts_canonical_sort_key_date_and_none() {
    let ledger = ContentLedger::new(vec![
        input(Some("2024-02-29"), "writing/leap.md"),
        input(None, "misc/undated.txt"),
    ])
    .unwrap();
    ledger.validate().unwrap();
}

#[test]
fn ledger_validation_rejects_malformed_sort_key_dates() {
    for date in [
        "",
        "2026-4-01",
        "2026-04-1",
        "20260401",
        "2026/04/01",
        "2026-04-01T12:00:00Z",
        "2026-04-01\n",
        "2026-00-01",
        "2026-13-01",
        "2026-04-00",
        "2026-04-31",
        "2026-02-29",
    ] {
        let ledger = ContentLedger::new(vec![input(Some(date), "writing/bad-date.md")]).unwrap();
        assert!(
            matches!(
                ledger.validate().unwrap_err(),
                LedgerValidationError::InvalidSortKeyDate { .. }
            ),
            "date {date:?} should fail validation"
        );
    }
}

#[test]
fn ledger_assigns_canonical_order_heights_and_chain_links() {
    let ledger = ContentLedger::new(vec![
        input(Some("2026-04-01"), "writing/z.md"),
        input(None, "misc/b.txt"),
        input(Some("2026-01-15"), "projects/a.md"),
        input(None, "misc/a.txt"),
    ])
    .unwrap();
    ledger.validate().unwrap();

    let paths = ledger
        .blocks
        .iter()
        .map(|block| block.entry.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec!["misc/a.txt", "misc/b.txt", "projects/a.md", "writing/z.md"]
    );
    assert_eq!(ledger.blocks[0].height, 1);
    assert_eq!(ledger.blocks[1].height, 2);
    assert_eq!(ledger.blocks[0].prev_block_sha256, ledger.genesis_hash);
    assert_eq!(
        ledger.blocks[1].prev_block_sha256,
        ledger.blocks[0].block_sha256
    );
    assert_eq!(
        ledger.chain_head,
        ledger.blocks.last().unwrap().block_sha256
    );
}

#[test]
fn empty_ledger_head_points_to_genesis() {
    let ledger = ContentLedger::new(Vec::new()).unwrap();
    ledger.validate().unwrap();
    assert_eq!(ledger.block_count, 0);
    assert_eq!(ledger.chain_head, ledger.genesis_hash);
}

#[test]
fn ledger_validation_rejects_duplicate_routes_ids_and_paths() {
    let mut duplicate_route = entry("projects/b.md");
    duplicate_route.route = "/projects/a.md".to_string();
    duplicate_route.id = "route:/other".to_string();
    let ledger = ContentLedger::new(vec![
        ContentLedgerInput::new(
            ContentLedgerSortKey::new(None, "projects/a.md".to_string()),
            entry("projects/a.md"),
        ),
        ContentLedgerInput::new(
            ContentLedgerSortKey::new(Some("2026-01-01".to_string()), "projects/b.md".to_string()),
            duplicate_route,
        ),
    ])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::DuplicateRoute { .. }
    ));

    let mut duplicate_id = entry("projects/b.md");
    duplicate_id.id = "route:/projects/a.md".to_string();
    let ledger = ContentLedger::new(vec![
        input(None, "projects/a.md"),
        ContentLedgerInput::new(
            ContentLedgerSortKey::new(Some("2026-01-01".to_string()), "projects/b.md".to_string()),
            duplicate_id,
        ),
    ])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::DuplicateId { .. }
    ));

    let mut duplicate_path = entry("projects/b.md");
    duplicate_path.path = "projects/a.md".to_string();
    let ledger = ContentLedger::new(vec![
        input(None, "projects/a.md"),
        ContentLedgerInput::new(
            ContentLedgerSortKey::new(Some("2026-01-01".to_string()), "projects/a.md".to_string()),
            duplicate_path,
        ),
    ])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::DuplicatePath { .. }
    ));
}

#[test]
fn ledger_validation_rejects_bad_id_route_and_path() {
    let mut bad_id = entry("writing/hello.md");
    bad_id.id = "content:writing/hello.md".to_string();
    let ledger = ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(None, "writing/hello.md".to_string()),
        bad_id,
    )])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::IdMismatch { .. }
    ));

    let mut bad_route = entry("writing/hello.md");
    bad_route.route = "writing/hello.md".to_string();
    let ledger = ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(None, "writing/hello.md".to_string()),
        bad_route,
    )])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::InvalidRoute { .. }
    ));

    let mut bad_path = entry("writing/hello.md");
    bad_path.path = "/writing/hello.md".to_string();
    let ledger = ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(None, "/writing/hello.md".to_string()),
        bad_path,
    )])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::InvalidContentPath { .. }
    ));
}

#[test]
fn ledger_validation_rejects_missing_primary_and_unsorted_content_files() {
    let missing_primary = ContentLedgerEntry::new(
        "route:/writing/hello.md".to_string(),
        "/writing/hello.md".to_string(),
        "writing/hello.md".to_string(),
        ContentLedgerCategory::Writing,
        vec![ContentFile {
            path: "content/writing/hello.meta.json".to_string(),
            sha256: sha('d'),
            bytes: 4,
        }],
    )
    .unwrap();
    let ledger = ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(None, "writing/hello.md".to_string()),
        missing_primary,
    )])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::MissingPrimaryContentFile { .. }
    ));

    let unsorted = ContentLedgerEntry::new(
        "route:/talks/a.pdf".to_string(),
        "/talks/a.pdf".to_string(),
        "talks/a.pdf".to_string(),
        ContentLedgerCategory::Talks,
        vec![
            ContentFile {
                path: "content/talks/a.pdf".to_string(),
                sha256: sha('e'),
                bytes: 3,
            },
            ContentFile {
                path: "content/talks/a.meta.json".to_string(),
                sha256: sha('f'),
                bytes: 4,
            },
        ],
    )
    .unwrap();
    let ledger = ContentLedger::new(vec![ContentLedgerInput::new(
        ContentLedgerSortKey::new(None, "talks/a.pdf".to_string()),
        unsorted,
    )])
    .unwrap();
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::ContentFilesNotSorted { .. }
    ));
}

#[test]
fn ledger_validation_rejects_tampering() {
    let mut ledger = ContentLedger::new(vec![
        input(Some("2026-01-01"), "writing/a.md"),
        input(Some("2026-02-01"), "projects/b.md"),
    ])
    .unwrap();
    ledger.blocks[0].entry.content_files[0].sha256 = sha('b');
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::ContentHashMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![
        input(Some("2026-01-01"), "writing/a.md"),
        input(Some("2026-02-01"), "projects/b.md"),
    ])
    .unwrap();
    ledger.blocks[1].prev_block_sha256 = sha('c');
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::PrevBlockHashMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![input(None, "writing/a.md")]).unwrap();
    ledger.blocks[0].block_sha256 = sha('d');
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::BlockHashMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![
        input(Some("2026-01-01"), "writing/a.md"),
        input(Some("2026-02-01"), "projects/b.md"),
    ])
    .unwrap();
    ledger.blocks.swap(0, 1);
    assert!(ledger.validate().is_err());

    let mut ledger = ContentLedger::new(vec![input(None, "writing/a.md")]).unwrap();
    ledger.blocks[0].height = 2;
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::BlockHeightMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![input(None, "writing/a.md")]).unwrap();
    ledger.blocks[0].entry.category = ContentLedgerCategory::Projects;
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::CategoryMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![input(None, "writing/a.md")]).unwrap();
    ledger.block_count = 2;
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::BlockCountMismatch { .. }
    ));

    let mut ledger = ContentLedger::new(vec![input(None, "writing/a.md")]).unwrap();
    ledger.chain_head = sha('e');
    assert!(matches!(
        ledger.validate().unwrap_err(),
        LedgerValidationError::ChainHeadMismatch
    ));
}

#[test]
fn non_current_ledger_shape_is_rejected() {
    let invalid = json!({
        "version": 1,
        "scheme": "websh.content-ledger.v1",
        "hash": "sha256",
        "entries": [],
        "entry_count": 0,
        "ledger_sha256": CONTENT_LEDGER_GENESIS_HASH,
    });
    assert!(serde_json::from_value::<ContentLedger>(invalid).is_err());
}
