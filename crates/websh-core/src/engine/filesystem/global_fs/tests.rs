use std::collections::HashMap;

use crate::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
use crate::ports::{ScannedDirectory, ScannedFile, ScannedSubtree};

use super::*;

fn file_meta(kind: NodeKind) -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind,
        bundle: None,
        authored: Fields::default(),
        derived: Fields::default(),
    }
}

fn dir_meta(name: &str) -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields {
            title: if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
            ..Fields::default()
        },
        derived: Fields::default(),
    }
}

fn snapshot(files: &[&str], directories: &[&str]) -> ScannedSubtree {
    ScannedSubtree {
        files: files
            .iter()
            .map(|path| ScannedFile {
                path: (*path).to_string(),
                meta: file_meta(NodeKind::Asset),
                extensions: EntryExtensions::default(),
            })
            .collect(),
        directories: directories
            .iter()
            .map(|path| ScannedDirectory {
                path: (*path).to_string(),
                meta: dir_meta(path.rsplit('/').next().unwrap_or(path)),
            })
            .collect(),
    }
}

#[test]
fn mounts_scanned_subtrees_under_canonical_prefixes() {
    let mut global = GlobalFs::empty();
    let site = snapshot(&["index.html", "about.md"], &["blog"]);
    let db = snapshot(&["notes/todo.md"], &["notes"]);

    global
        .mount_scanned_subtree(VirtualPath::root(), &site)
        .unwrap();
    global
        .mount_scanned_subtree(VirtualPath::from_absolute("/db").unwrap(), &db)
        .unwrap();

    assert!(
        global
            .get_entry(&VirtualPath::from_absolute("/index.html").unwrap())
            .is_some()
    );
    assert!(
        global
            .get_entry(&VirtualPath::from_absolute("/db/notes/todo.md").unwrap())
            .is_some()
    );
}

#[test]
fn mutation_rejects_file_ancestor_without_replacing_it() {
    let mut global = GlobalFs::empty();
    let parent = VirtualPath::from_absolute("/a").unwrap();
    global.upsert_file(
        parent.clone(),
        String::new(),
        file_meta(NodeKind::Asset),
        EntryExtensions::default(),
    );

    let err = global
        .try_upsert_file(
            VirtualPath::from_absolute("/a/b.md").unwrap(),
            "child".to_string(),
            file_meta(NodeKind::Asset),
            EntryExtensions::default(),
        )
        .unwrap_err();

    assert_eq!(
        err,
        FsMutationError::ParentIsFile {
            path: parent.clone()
        }
    );
    assert!(
        global
            .get_entry(&parent)
            .is_some_and(|entry| !entry.is_directory())
    );
    assert!(
        global
            .get_entry(&VirtualPath::from_absolute("/a/b.md").unwrap())
            .is_none()
    );
    assert!(
        global
            .read_pending_text(&VirtualPath::from_absolute("/a/b.md").unwrap())
            .is_none()
    );
}

#[test]
fn refuses_to_replace_existing_directory_mountpoint() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(
            VirtualPath::from_absolute("/db").unwrap(),
            &snapshot(&["index.md"], &[]),
        )
        .unwrap();

    let err = global
        .mount_scanned_subtree(
            VirtualPath::from_absolute("/db").unwrap(),
            &snapshot(&["other.md"], &[]),
        )
        .unwrap_err();

    assert_eq!(
        err,
        MountError::MountPointOccupied {
            path: VirtualPath::from_absolute("/db").unwrap()
        }
    );
}

#[test]
fn reserves_mount_point_as_empty_export_exclusion() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(
            VirtualPath::root(),
            &snapshot(&["index.md", "mempool/root-fallback.md"], &["mempool"]),
        )
        .unwrap();
    global
        .reserve_mount_point(VirtualPath::from_absolute("/mempool").unwrap())
        .unwrap();

    assert!(global.is_directory(&VirtualPath::from_absolute("/mempool").unwrap()));
    assert!(
        global
            .get_entry(&VirtualPath::from_absolute("/mempool/root-fallback.md").unwrap())
            .is_none()
    );

    let root_snapshot = global.export_mount_snapshot(&VirtualPath::root()).unwrap();
    let files = root_snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(files, vec!["index.md"]);
}

#[test]
fn replaces_reserved_mount_point_with_scanned_subtree() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(VirtualPath::root(), &snapshot(&["index.md"], &[]))
        .unwrap();
    let db_root = VirtualPath::from_absolute("/db").unwrap();
    global.reserve_mount_point(db_root.clone()).unwrap();
    global
        .replace_scanned_subtree(db_root.clone(), &snapshot(&["notes/todo.md"], &["notes"]))
        .unwrap();

    assert!(
        global
            .get_entry(&VirtualPath::from_absolute("/db/notes/todo.md").unwrap())
            .is_some()
    );

    let root_snapshot = global.export_mount_snapshot(&VirtualPath::root()).unwrap();
    assert!(
        root_snapshot
            .files
            .iter()
            .all(|file| !file.path.starts_with("db/"))
    );
    let db_snapshot = global.export_mount_snapshot(&db_root).unwrap();
    assert_eq!(db_snapshot.files[0].path, "notes/todo.md");
}

#[test]
fn remounting_root_replaces_mount_registry() {
    let mut global = GlobalFs::empty();
    global
        .mount_subtree(
            VirtualPath::root(),
            FsEntry::Directory {
                children: HashMap::new(),
                meta: dir_meta(""),
            },
        )
        .unwrap();

    let points: Vec<_> = global
        .mount_points()
        .map(|p| p.as_str().to_string())
        .collect();
    assert_eq!(points, vec!["/"]);
}

#[test]
fn list_dir_uses_global_absolute_paths() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(
            VirtualPath::root(),
            &snapshot(&["blog/hello.md"], &["blog"]),
        )
        .unwrap();

    let entries = global
        .list_dir(&VirtualPath::from_absolute("/blog").unwrap())
        .unwrap();

    assert_eq!(entries[0].path.as_str(), "/blog/hello.md");
}

#[test]
fn child_summary_avoids_full_dir_entry_materialization() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(
            VirtualPath::root(),
            &snapshot(&["blog/hello.md", "blog/zeta.md"], &["blog", "blog/assets"]),
        )
        .unwrap();
    let blog = VirtualPath::from_absolute("/blog").unwrap();

    assert_eq!(global.child_count(&blog), Some(3));
    assert_eq!(
        global.child_names(&blog),
        Some(vec![
            "assets".to_string(),
            "hello.md".to_string(),
            "zeta.md".to_string()
        ])
    );
}

#[test]
fn pending_text_tracks_upserts() {
    let mut global = GlobalFs::empty();
    let path = VirtualPath::from_absolute("/new.md").unwrap();
    global.upsert_file(
        path.clone(),
        "hello".to_string(),
        file_meta(NodeKind::Page),
        EntryExtensions::default(),
    );

    assert_eq!(global.read_pending_text(&path).as_deref(), Some("hello"));
}

#[test]
fn scanned_subtree_roundtrip_is_byte_stable() {
    let golden = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/manifest_golden.json"
    ));
    let snapshot = crate::ports::parse_manifest_snapshot(golden).expect("golden parses");

    let mut global = GlobalFs::empty();
    let root = VirtualPath::root();
    global
        .mount_scanned_subtree(root.clone(), &snapshot)
        .unwrap();
    let reserialized = global.export_mount_snapshot(&root).unwrap();
    let out = crate::ports::serialize_manifest_snapshot(&reserialized).expect("serialize");

    assert_eq!(out.trim_end(), golden.trim_end());
}

#[test]
fn exported_mount_snapshot_sorts_regardless_of_input_order() {
    let tagged_dir = |title: &str, tag: &str| NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields {
            title: Some(title.to_string()),
            tags: Some(vec![tag.to_string()]),
            ..Fields::default()
        },
        derived: Fields::default(),
    };

    let snapshot = ScannedSubtree {
        files: vec![
            ScannedFile {
                path: "z.md".to_string(),
                meta: file_meta(NodeKind::Page),
                extensions: EntryExtensions::default(),
            },
            ScannedFile {
                path: "m.md".to_string(),
                meta: file_meta(NodeKind::Page),
                extensions: EntryExtensions::default(),
            },
            ScannedFile {
                path: "a.md".to_string(),
                meta: file_meta(NodeKind::Page),
                extensions: EntryExtensions::default(),
            },
        ],
        directories: vec![
            ScannedDirectory {
                path: "z-dir".to_string(),
                meta: tagged_dir("Z", "zone"),
            },
            ScannedDirectory {
                path: "a-dir".to_string(),
                meta: tagged_dir("A", "area"),
            },
        ],
    };

    let mut global = GlobalFs::empty();
    let root = VirtualPath::root();
    global
        .mount_scanned_subtree(root.clone(), &snapshot)
        .unwrap();
    let out = global.export_mount_snapshot(&root).unwrap();
    let file_paths: Vec<&str> = out.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(file_paths, vec!["a.md", "m.md", "z.md"]);
    let dir_paths: Vec<&str> = out.directories.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(dir_paths, vec!["a-dir", "z-dir"]);
}

#[test]
fn exported_mount_snapshot_uses_relative_paths_for_pending_files() {
    let mut global = GlobalFs::empty();
    let root = VirtualPath::root();
    global
        .mount_scanned_subtree(root.clone(), &ScannedSubtree::default())
        .unwrap();
    global.upsert_file(
        root.join("notes.md"),
        "notes".into(),
        file_meta(NodeKind::Page),
        EntryExtensions::default(),
    );

    let snapshot = global.export_mount_snapshot(&root).unwrap();
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].path, "notes.md");
}

#[test]
fn exported_mount_snapshot_preserves_empty_directories() {
    let mut global = GlobalFs::empty();
    let root = VirtualPath::root();
    global
        .mount_scanned_subtree(root.clone(), &ScannedSubtree::default())
        .unwrap();
    global.upsert_directory(root.join("empty"), dir_meta("empty"));

    let snapshot = global.export_mount_snapshot(&root).unwrap();
    let paths: Vec<_> = snapshot
        .directories
        .iter()
        .map(|dir| dir.path.as_str())
        .collect();
    assert_eq!(paths, vec!["empty"]);
}

#[test]
fn root_export_excludes_descendant_mounts_and_runtime_state() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(
            VirtualPath::root(),
            &snapshot(
                &[
                    "index.md",
                    ".websh/site.json",
                    ".websh/mounts/db.mount.json",
                ],
                &[".websh", ".websh/mounts"],
            ),
        )
        .unwrap();
    global
        .mount_scanned_subtree(
            VirtualPath::from_absolute("/db").unwrap(),
            &snapshot(&["fresh.md"], &[]),
        )
        .unwrap();
    global.upsert_directory(
        VirtualPath::from_absolute("/.websh/state").unwrap(),
        dir_meta("state"),
    );
    global.upsert_file(
        VirtualPath::from_absolute("/.websh/state/session/wallet_session").unwrap(),
        "1".into(),
        file_meta(NodeKind::Data),
        EntryExtensions::default(),
    );

    let snapshot = global.export_mount_snapshot(&VirtualPath::root()).unwrap();
    let files: Vec<_> = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();

    assert!(files.contains(&"index.md"));
    assert!(files.contains(&".websh/site.json"));
    assert!(files.contains(&".websh/mounts/db.mount.json"));
    assert!(!files.iter().any(|path| path.starts_with("db/")));
    assert!(!files.iter().any(|path| path.starts_with(".websh/state/")));
}

#[test]
fn descendant_mount_export_includes_only_mount_relative_files() {
    let mut global = GlobalFs::empty();
    global
        .mount_scanned_subtree(VirtualPath::root(), &snapshot(&["index.md"], &[]))
        .unwrap();
    let db_root = VirtualPath::from_absolute("/db").unwrap();
    global
        .mount_scanned_subtree(db_root.clone(), &snapshot(&["fresh.md"], &[]))
        .unwrap();

    let snapshot = global.export_mount_snapshot(&db_root).unwrap();
    let files: Vec<_> = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();

    assert_eq!(files, vec!["fresh.md"]);
}
