use crate::domain::{
    ChangeSet, ChangeType, EntryExtensions, Fields, NodeKind, NodeMetadata, RuntimeMount,
    SCHEMA_VERSION, VirtualPath, WalletState,
};
use crate::engine::filesystem::GlobalFs;
use crate::engine::shell::{AccessPolicy, CommandResult, PathArg, SideEffect};

use super::{require_write_access, resolve_path_arg};

pub(super) struct WriteCommandContext<'a> {
    pub(super) wallet_state: &'a WalletState,
    pub(super) access_policy: &'a AccessPolicy,
    pub(super) runtime_mounts: &'a [RuntimeMount],
    pub(super) fs: &'a GlobalFs,
    pub(super) cwd: &'a VirtualPath,
    pub(super) changes: &'a ChangeSet,
}

pub(super) fn blank_file_meta(kind: NodeKind) -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind,
        bundle: None,
        authored: Fields::default(),
        derived: Fields::default(),
    }
}

pub(super) fn blank_dir_meta() -> NodeMetadata {
    NodeMetadata {
        schema: SCHEMA_VERSION,
        kind: NodeKind::Directory,
        bundle: None,
        authored: Fields::default(),
        derived: Fields::default(),
    }
}

#[allow(clippy::result_large_err)]
fn resolve_abs_path(
    cmd_label: &str,
    path: &PathArg,
    cwd: &VirtualPath,
) -> Result<VirtualPath, CommandResult> {
    resolve_path_arg(cmd_label, path.as_str(), cwd)
}

/// Execute `touch` — create an empty file.
pub(super) fn execute_touch(
    path: PathArg,
    wallet_state: &WalletState,
    access_policy: &AccessPolicy,
    runtime_mounts: &[RuntimeMount],
    fs: &GlobalFs,
    cwd: &VirtualPath,
) -> CommandResult {
    let vp = match resolve_abs_path("touch", &path, cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access("touch", wallet_state, access_policy, runtime_mounts, &vp)
    {
        return e;
    }

    if fs.exists(&vp) {
        return CommandResult::error_line(format!("touch: {}: path already exists", path));
    }

    if let Err(e) = require_parent_directory("touch", &path, fs, &vp) {
        return e;
    }

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::ApplyChange {
            path: vp,
            change: Box::new(ChangeType::CreateFile {
                content: String::new(),
                meta: blank_file_meta(NodeKind::Asset),
                extensions: EntryExtensions::default(),
            }),
        }],
    }
}

/// Execute `mkdir` — create a directory.
pub(super) fn execute_mkdir(
    path: PathArg,
    wallet_state: &WalletState,
    access_policy: &AccessPolicy,
    runtime_mounts: &[RuntimeMount],
    fs: &GlobalFs,
    cwd: &VirtualPath,
) -> CommandResult {
    let vp = match resolve_abs_path("mkdir", &path, cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access("mkdir", wallet_state, access_policy, runtime_mounts, &vp)
    {
        return e;
    }

    if fs.exists(&vp) {
        return CommandResult::error_line(format!("mkdir: {}: path already exists", path));
    }

    if let Err(e) = require_parent_directory("mkdir", &path, fs, &vp) {
        return e;
    }

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::ApplyChange {
            path: vp,
            change: Box::new(ChangeType::CreateDirectory {
                meta: blank_dir_meta(),
            }),
        }],
    }
}

/// Execute `rm` — delete a file or directory (with `-r` for directories).
pub(super) fn execute_rm(
    path: PathArg,
    recursive: bool,
    ctx: WriteCommandContext<'_>,
) -> CommandResult {
    let vp = match resolve_abs_path("rm", &path, ctx.cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access(
        "rm",
        ctx.wallet_state,
        ctx.access_policy,
        ctx.runtime_mounts,
        &vp,
    ) {
        return e;
    }

    let Some(entry) = ctx.fs.get_entry(&vp) else {
        return CommandResult::error_line(format!("rm: {}: no such file or directory", path));
    };

    if entry.is_directory() && !recursive {
        return CommandResult::error_line(format!("rm: {}: is a directory (use -r)", path));
    }

    if entry.is_directory() && is_runtime_mount_root(ctx.runtime_mounts, &vp) {
        return CommandResult::error_line(format!("rm: {}: cannot remove mount root", path));
    }

    if is_pending_create(ctx.changes, &vp) {
        return CommandResult {
            output: vec![],
            exit_code: 0,
            side_effects: vec![SideEffect::DiscardChange { path: vp }],
        };
    }

    let change = if entry.is_directory() {
        ChangeType::DeleteDirectory
    } else {
        ChangeType::DeleteFile
    };

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::ApplyChange {
            path: vp,
            change: Box::new(change),
        }],
    }
}

/// Execute `rmdir` — delete an empty directory.
pub(super) fn execute_rmdir(
    path: PathArg,
    wallet_state: &WalletState,
    access_policy: &AccessPolicy,
    runtime_mounts: &[RuntimeMount],
    fs: &GlobalFs,
    cwd: &VirtualPath,
    changes: &ChangeSet,
) -> CommandResult {
    let vp = match resolve_abs_path("rmdir", &path, cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access("rmdir", wallet_state, access_policy, runtime_mounts, &vp)
    {
        return e;
    }

    let Some(entry) = fs.get_entry(&vp) else {
        return CommandResult::error_line(format!("rmdir: {}: no such file or directory", path));
    };

    if !entry.is_directory() {
        return CommandResult::error_line(format!("rmdir: {}: not a directory", path));
    }

    if is_runtime_mount_root(runtime_mounts, &vp) {
        return CommandResult::error_line(format!("rmdir: {}: cannot remove mount root", path));
    }

    if fs.has_children(&vp) {
        return CommandResult::error_line(format!("rmdir: {}: directory not empty", path));
    }

    if is_pending_create(changes, &vp) {
        return CommandResult {
            output: vec![],
            exit_code: 0,
            side_effects: vec![SideEffect::DiscardChange { path: vp }],
        };
    }

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::ApplyChange {
            path: vp,
            change: Box::new(ChangeType::DeleteDirectory),
        }],
    }
}

fn is_runtime_mount_root(runtime_mounts: &[RuntimeMount], path: &VirtualPath) -> bool {
    runtime_mounts.iter().any(|mount| mount.root == *path)
}

fn is_pending_create(changes: &ChangeSet, path: &VirtualPath) -> bool {
    matches!(
        changes.get(path).map(|e| &e.change),
        Some(
            ChangeType::CreateFile { .. }
                | ChangeType::CreateBinary { .. }
                | ChangeType::CreateDirectory { .. }
        )
    )
}

/// Execute `edit` — request the editor UI open for a file.
pub(super) fn execute_edit(
    path: PathArg,
    wallet_state: &WalletState,
    access_policy: &AccessPolicy,
    runtime_mounts: &[RuntimeMount],
    fs: &GlobalFs,
    cwd: &VirtualPath,
) -> CommandResult {
    let vp = match resolve_abs_path("edit", &path, cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access("edit", wallet_state, access_policy, runtime_mounts, &vp) {
        return e;
    }

    if let Some(entry) = fs.get_entry(&vp)
        && entry.is_directory()
    {
        return CommandResult::error_line(format!("edit: {}: is a directory", path));
    }

    if !fs.exists(&vp)
        && let Err(e) = require_parent_directory("edit", &path, fs, &vp)
    {
        return e;
    }

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::OpenEditor { path: vp }],
    }
}

/// Execute `echo "..." > path` — create or update a file with literal content.
pub(super) fn execute_echo_redirect(
    body: String,
    path: PathArg,
    wallet_state: &WalletState,
    access_policy: &AccessPolicy,
    runtime_mounts: &[RuntimeMount],
    fs: &GlobalFs,
    cwd: &VirtualPath,
) -> CommandResult {
    let vp = match resolve_abs_path("echo", &path, cwd) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if let Err(e) = require_write_access("echo", wallet_state, access_policy, runtime_mounts, &vp) {
        return e;
    }

    let change = match fs.get_entry(&vp) {
        Some(entry) if entry.is_directory() => {
            return CommandResult::error_line(format!("echo: {}: is a directory", path));
        }
        Some(_) => ChangeType::UpdateFile {
            content: body,
            meta: None,
            extensions: None,
        },
        None => {
            if let Err(e) = require_parent_directory("echo", &path, fs, &vp) {
                return e;
            }
            ChangeType::CreateFile {
                content: body,
                meta: blank_file_meta(NodeKind::Asset),
                extensions: EntryExtensions::default(),
            }
        }
    };

    CommandResult {
        output: vec![],
        exit_code: 0,
        side_effects: vec![SideEffect::ApplyChange {
            path: vp,
            change: Box::new(change),
        }],
    }
}

#[allow(clippy::result_large_err)]
fn require_parent_directory(
    cmd_label: &str,
    original: &PathArg,
    fs: &GlobalFs,
    path: &VirtualPath,
) -> Result<(), CommandResult> {
    let Some(parent) = path.parent() else {
        return Err(CommandResult::error_line(format!(
            "{}: {}: cannot create filesystem root",
            cmd_label, original
        )));
    };

    match fs.get_entry(&parent) {
        Some(entry) if entry.is_directory() => Ok(()),
        Some(_) => Err(CommandResult::error_line(format!(
            "{}: {}: parent is not a directory",
            cmd_label, original
        ))),
        None => Err(CommandResult::error_line(format!(
            "{}: {}: parent directory does not exist",
            cmd_label, original
        ))),
    }
}
