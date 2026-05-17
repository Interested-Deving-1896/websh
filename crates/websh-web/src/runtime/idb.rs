//! IndexedDB persistence for browser runtime state.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use idb::event::{DatabaseEvent, VersionChangeEvent};
use idb::{Database, Event, Factory, ObjectStoreParams, Request, TransactionMode};
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::Serializer;
use wasm_bindgen::JsValue;
use websh_core::domain::{ChangeEntry, ChangeSet, VirtualPath};
use websh_core::ports::{StorageError, StorageResult};

const DB_NAME: &str = "websh-state";
const DB_VERSION: u32 = 3;
const DRAFT_PATHS_METADATA_PREFIX: &str = "draft_paths:";
pub const STORE_DRAFT_CHANGES: &str = "draft_changes";
pub const STORE_METADATA: &str = "metadata";

#[derive(Serialize, Deserialize)]
struct DraftChangeRecord {
    key: String,
    draft_id: String,
    path: String,
    entry: ChangeEntry,
}

#[derive(Serialize, Deserialize)]
struct MetadataRecord {
    key: String,
    value: String,
}

pub async fn open_db() -> StorageResult<Database> {
    let factory = Factory::new().map_err(idb_err)?;
    let mut req = factory.open(DB_NAME, Some(DB_VERSION)).map_err(idb_err)?;
    let upgrade_error = Rc::new(RefCell::new(None));
    let upgrade_error_for_callback = upgrade_error.clone();

    req.on_upgrade_needed(move |event| {
        if let Err(error) = upgrade_schema(&event) {
            abort_upgrade(&event);
            *upgrade_error_for_callback.borrow_mut() = Some(error);
        }
    });

    let db = req.await.map_err(idb_err)?;
    if let Some(error) = upgrade_error.borrow_mut().take() {
        return Err(error);
    }
    validate_required_stores(&db)?;
    Ok(db)
}

fn upgrade_schema(event: &VersionChangeEvent) -> StorageResult<()> {
    let db = event.database().map_err(idb_err)?;
    if db.store_names().iter().any(|name| name == "drafts") {
        db.delete_object_store("drafts").map_err(idb_err)?;
    }
    ensure_store(&db, STORE_DRAFT_CHANGES, "key")?;
    ensure_store(&db, STORE_METADATA, "key")?;
    Ok(())
}

fn abort_upgrade(event: &VersionChangeEvent) {
    if let Ok(request) = event.target()
        && let Some(tx) = request.transaction()
    {
        let _ = tx.abort();
    }
}

fn validate_required_stores(db: &Database) -> StorageResult<()> {
    for store_name in [STORE_DRAFT_CHANGES, STORE_METADATA] {
        if !db.store_names().iter().any(|name| name == store_name) {
            return Err(StorageError::InvalidRequest {
                message: format!(
                    "idb schema missing object store `{store_name}`. clear site data to recreate local storage"
                ),
            });
        }
    }
    Ok(())
}

fn ensure_store(db: &Database, store_name: &str, key_path: &str) -> StorageResult<()> {
    if db.store_names().iter().any(|name| name == store_name) {
        return Ok(());
    }

    let mut params = ObjectStoreParams::new();
    params.key_path(Some(idb::KeyPath::new_single(key_path)));
    db.create_object_store(store_name, params)
        .map_err(idb_err)?;
    Ok(())
}

#[cfg(all(test, target_arch = "wasm32"))]
async fn save_draft(db: &Database, draft_id: &str, changes: &ChangeSet) -> StorageResult<()> {
    let previous = load_draft(db, draft_id).await?.unwrap_or_default();
    save_draft_delta(db, draft_id, &previous, changes).await
}

pub async fn save_draft_delta(
    db: &Database,
    draft_id: &str,
    previous: &ChangeSet,
    current: &ChangeSet,
) -> StorageResult<()> {
    let current_path_set = current
        .iter_all()
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    let deletes = previous
        .iter_all()
        .filter(|(path, _)| !current_path_set.contains(*path))
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let upserts = current
        .iter_all()
        .filter(|(path, entry)| previous.get(path) != Some(*entry))
        .map(|(path, entry)| (path.clone(), entry.clone()))
        .collect::<Vec<_>>();
    let current_paths = current_path_set
        .iter()
        .map(|path| path.as_str().to_string())
        .collect::<Vec<_>>();

    if deletes.is_empty() && upserts.is_empty() {
        return Ok(());
    }

    let tx = db
        .transaction(
            &[STORE_DRAFT_CHANGES, STORE_METADATA],
            TransactionMode::ReadWrite,
        )
        .map_err(idb_err)?;
    let changes_store = tx.object_store(STORE_DRAFT_CHANGES).map_err(idb_err)?;
    let metadata_store = tx.object_store(STORE_METADATA).map_err(idb_err)?;

    for stale_path in deletes {
        changes_store
            .delete(JsValue::from_str(&draft_change_key(
                draft_id,
                stale_path.as_str(),
            )))
            .map_err(idb_err)?
            .await
            .map_err(idb_err)?;
    }

    for (path, entry) in upserts {
        let record = DraftChangeRecord {
            key: draft_change_key(draft_id, path.as_str()),
            draft_id: draft_id.to_string(),
            path: path.as_str().to_string(),
            entry,
        };
        let value = record
            .serialize(&Serializer::json_compatible())
            .map_err(|e| StorageError::InvalidRequest {
                message: format!("serialize: {e}"),
            })?;
        changes_store
            .put(&value, None)
            .map_err(idb_err)?
            .await
            .map_err(idb_err)?;
    }

    let index_record = MetadataRecord {
        key: draft_paths_metadata_key(draft_id),
        value: serde_json::to_string(&current_paths).map_err(|e| StorageError::InvalidRequest {
            message: format!("serialize draft index: {e}"),
        })?,
    };
    let index_value = index_record
        .serialize(&Serializer::json_compatible())
        .map_err(|e| StorageError::InvalidRequest {
            message: format!("serialize: {e}"),
        })?;
    metadata_store
        .put(&index_value, None)
        .map_err(idb_err)?
        .await
        .map_err(idb_err)?;

    tx.commit().map_err(idb_err)?.await.map_err(idb_err)?;
    Ok(())
}

pub async fn load_draft(db: &Database, draft_id: &str) -> StorageResult<Option<ChangeSet>> {
    if let Some(paths) = load_draft_path_index(db, draft_id).await? {
        return load_pathwise_draft(db, draft_id, paths).await.map(Some);
    }
    Ok(None)
}

async fn load_pathwise_draft(
    db: &Database,
    draft_id: &str,
    paths: Vec<String>,
) -> StorageResult<ChangeSet> {
    let tx = db
        .transaction(&[STORE_DRAFT_CHANGES], TransactionMode::ReadOnly)
        .map_err(idb_err)?;
    let store = tx.object_store(STORE_DRAFT_CHANGES).map_err(idb_err)?;
    let mut changes = ChangeSet::new();

    for path in paths {
        let value: Option<JsValue> = store
            .get(JsValue::from_str(&draft_change_key(draft_id, &path)))
            .map_err(idb_err)?
            .await
            .map_err(idb_err)?;
        let Some(value) = value else { continue };
        let record: DraftChangeRecord =
            serde_wasm_bindgen::from_value(value).map_err(|e| StorageError::InvalidRequest {
                message: format!("deserialize: {e}"),
            })?;
        let path = VirtualPath::from_absolute(record.path).map_err(|error| {
            StorageError::InvalidRequest {
                message: error.to_string(),
            }
        })?;
        let entry = record.entry;
        changes.upsert_at(path.clone(), entry.change, entry.timestamp);
        if !entry.staged {
            changes.unstage(&path);
        }
    }

    Ok(changes)
}

async fn load_draft_path_index(
    db: &Database,
    draft_id: &str,
) -> StorageResult<Option<Vec<String>>> {
    let Some(raw) = load_metadata(db, &draft_paths_metadata_key(draft_id)).await? else {
        return Ok(None);
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| StorageError::InvalidRequest {
            message: format!("deserialize draft index: {e}"),
        })
}

fn draft_paths_metadata_key(draft_id: &str) -> String {
    format!("{DRAFT_PATHS_METADATA_PREFIX}{draft_id}")
}

fn draft_change_key(draft_id: &str, path: &str) -> String {
    format!("{draft_id}:{path}")
}

pub async fn save_metadata(db: &Database, key: &str, value: &str) -> StorageResult<()> {
    let tx = db
        .transaction(&[STORE_METADATA], TransactionMode::ReadWrite)
        .map_err(idb_err)?;
    let store = tx.object_store(STORE_METADATA).map_err(idb_err)?;
    let record = MetadataRecord {
        key: key.to_string(),
        value: value.to_string(),
    };
    let js = record
        .serialize(&Serializer::json_compatible())
        .map_err(|e| StorageError::InvalidRequest {
            message: format!("serialize: {e}"),
        })?;
    store
        .put(&js, None)
        .map_err(idb_err)?
        .await
        .map_err(idb_err)?;
    tx.commit().map_err(idb_err)?.await.map_err(idb_err)?;
    Ok(())
}

pub async fn load_metadata(db: &Database, key: &str) -> StorageResult<Option<String>> {
    let tx = db
        .transaction(&[STORE_METADATA], TransactionMode::ReadOnly)
        .map_err(idb_err)?;
    let store = tx.object_store(STORE_METADATA).map_err(idb_err)?;
    let value: Option<JsValue> = store
        .get(JsValue::from_str(key))
        .map_err(idb_err)?
        .await
        .map_err(idb_err)?;
    match value {
        None => Ok(None),
        Some(v) => {
            let record: MetadataRecord =
                serde_wasm_bindgen::from_value(v).map_err(|e| StorageError::InvalidRequest {
                    message: format!("deserialize: {e}"),
                })?;
            Ok(Some(record.value))
        }
    }
}

fn idb_err<E: std::fmt::Display>(e: E) -> StorageError {
    let s = e.to_string().to_lowercase();
    if s.contains("quotaexceeded") {
        StorageError::InvalidRequest {
            message: "local draft storage full. discard or commit to free space".into(),
        }
    } else {
        StorageError::Network {
            message: format!("idb: {e}"),
        }
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use wasm_bindgen_test::*;
    use websh_core::domain::{ChangeType, EntryExtensions, NodeMetadata, VirtualPath};

    use super::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn save_then_load_draft_preserves_content() {
        let db = open_db().await.expect("open db");
        let mut cs = ChangeSet::new();
        let p = VirtualPath::from_absolute("/rt.md").unwrap();
        cs.upsert_at(
            p.clone(),
            ChangeType::CreateFile {
                content: "roundtrip".into(),
                meta: NodeMetadata::default(),
                extensions: EntryExtensions::default(),
            },
            1234,
        );

        save_draft(&db, "test-mount", &cs).await.expect("save");
        let loaded = load_draft(&db, "test-mount")
            .await
            .expect("load")
            .expect("exists");

        let entry = loaded.get(&p).expect("entry present");
        match &entry.change {
            ChangeType::CreateFile { content, .. } => assert_eq!(content, "roundtrip"),
            _ => panic!("wrong variant"),
        }
    }

    #[wasm_bindgen_test]
    async fn pathwise_draft_save_removes_stale_entries() {
        let db = open_db().await.expect("open db");
        let draft_id = "test-pathwise-draft";
        let keep = VirtualPath::from_absolute("/keep.md").unwrap();
        let stale = VirtualPath::from_absolute("/stale.md").unwrap();

        let mut first = ChangeSet::new();
        first.upsert_at(
            keep.clone(),
            ChangeType::CreateFile {
                content: "keep".into(),
                meta: NodeMetadata::default(),
                extensions: EntryExtensions::default(),
            },
            1,
        );
        first.upsert_at(
            stale.clone(),
            ChangeType::CreateFile {
                content: "stale".into(),
                meta: NodeMetadata::default(),
                extensions: EntryExtensions::default(),
            },
            2,
        );
        save_draft(&db, draft_id, &first).await.expect("first save");

        let mut second = ChangeSet::new();
        second.upsert_at(
            keep.clone(),
            ChangeType::UpdateFile {
                content: "updated".into(),
                meta: None,
                extensions: None,
            },
            3,
        );
        save_draft(&db, draft_id, &second)
            .await
            .expect("second save");

        let loaded = load_draft(&db, draft_id)
            .await
            .expect("load")
            .expect("exists");
        assert!(loaded.get(&stale).is_none());
        match &loaded.get(&keep).expect("keep present").change {
            ChangeType::UpdateFile { content, .. } => assert_eq!(content, "updated"),
            _ => panic!("wrong variant"),
        }
    }
}
