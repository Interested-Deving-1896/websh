//! Tab autocomplete functionality for terminal commands and paths.
//!
//! This module provides intelligent autocompletion for:
//! - Command names (e.g., "cl" → "clear")
//! - Directory paths for `cd`, `ls` commands
//! - File paths for `cat` commands
//!
//! The autocomplete system supports:
//! - Single match: Complete immediately
//! - Multiple matches: Show common prefix and all options
//! - Ghost text hints while typing

use crate::domain::DirEntry;
use crate::domain::VirtualPath;
use crate::engine::filesystem::{GlobalFs, canonicalize_user_path};
use crate::engine::shell::Command;

/// Result of an autocomplete attempt.
#[derive(Clone, Debug, PartialEq)]
pub enum AutocompleteResult {
    /// Single exact match - complete with this value.
    Single(String),
    /// Multiple matches - (common_prefix, all_matches).
    Multiple(String, Vec<String>),
    /// No matches found.
    None,
}

/// Commands that accept directory paths as arguments.
const DIR_COMMANDS: &[&str] = &["cd", "ls", "mkdir", "rmdir"];

/// Commands that accept file paths as arguments.
///
/// These commands also match directories during tab completion so users
/// can drill into subdirectories — the filter just doesn't restrict to
/// directories only (unlike `DIR_COMMANDS`).
const FILE_COMMANDS: &[&str] = &["cat", "touch", "rm", "edit"];

/// Subcommands for `sync` (first positional arg).
const SYNC_SUBCOMMANDS: &[&str] = &["status", "commit", "refresh", "auth"];

/// Subcommands for `sync auth` (second positional arg).
const SYNC_AUTH_SUBCOMMANDS: &[&str] = &["set", "clear"];

/// Determines what type of completion is needed for a command.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CompletionMode {
    /// Complete command names only.
    Command,
    /// Complete directory paths (for cd, ls).
    DirectoryPath,
    /// Complete file paths (for cat).
    FilePath,
    /// No completion available.
    None,
}

impl CompletionMode {
    /// Determine completion mode from input.
    fn from_input(input: &str) -> (Self, Vec<&str>) {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();

        if parts.len() == 1 {
            return (Self::Command, parts);
        }

        let cmd_lower = parts[0].to_lowercase();
        let mode = if DIR_COMMANDS.contains(&cmd_lower.as_str()) {
            Self::DirectoryPath
        } else if FILE_COMMANDS.contains(&cmd_lower.as_str()) {
            Self::FilePath
        } else {
            Self::None
        };

        (mode, parts)
    }

    /// Returns true if this mode only matches directories.
    fn dirs_only(self) -> bool {
        matches!(self, Self::DirectoryPath)
    }
}

/// Parsed path components for autocomplete.
struct ParsedPath<'a> {
    /// Directory prefix (e.g., "projects/" or "").
    dir_part: &'a str,
    /// Filename/directory name being completed.
    name_part: &'a str,
    /// Resolved search directory path.
    search_dir: VirtualPath,
}

impl<'a> ParsedPath<'a> {
    /// Parse a partial path and resolve the search directory.
    fn parse(partial: &'a str, cwd: &VirtualPath, _fs: &GlobalFs) -> Option<Self> {
        let (dir_part, name_part) = match partial.rfind('/') {
            Some(idx) => (&partial[..=idx], &partial[idx + 1..]),
            None => ("", partial),
        };

        let search_dir = if dir_part.is_empty() {
            cwd.clone()
        } else {
            canonicalize_user_path(cwd, dir_part.trim_end_matches('/'))?
        };

        Some(Self {
            dir_part,
            name_part,
            search_dir,
        })
    }
}

/// Perform autocomplete on Tab press.
///
/// Returns a completion result based on the current input and filesystem state.
pub fn autocomplete(input: &str, cwd: &VirtualPath, fs: &GlobalFs) -> AutocompleteResult {
    let input = input.trim_start();
    if input.is_empty() {
        return AutocompleteResult::None;
    }

    let (mode, parts) = CompletionMode::from_input(input);

    // `sync` has its own subcommand grammar (not a path). Handle it before
    // the generic mode dispatch.
    if mode != CompletionMode::Command && parts[0].eq_ignore_ascii_case("sync") {
        return complete_sync(parts[1]);
    }

    match mode {
        CompletionMode::Command => complete_command(parts[0]),
        CompletionMode::DirectoryPath | CompletionMode::FilePath => {
            complete_path(parts[0], parts[1], cwd, fs, mode.dirs_only())
        }
        CompletionMode::None => AutocompleteResult::None,
    }
}

/// Get autocomplete suggestion for ghost text hint (while typing).
///
/// Returns the suffix that would complete the current input.
pub fn get_hint(input: &str, cwd: &VirtualPath, fs: &GlobalFs) -> Option<String> {
    let input = input.trim_start();
    if input.is_empty() {
        return None;
    }

    let (mode, parts) = CompletionMode::from_input(input);

    if mode != CompletionMode::Command && parts[0].eq_ignore_ascii_case("sync") {
        return get_sync_hint(parts[1]);
    }

    match mode {
        CompletionMode::Command => get_command_hint(parts[0]),
        CompletionMode::DirectoryPath | CompletionMode::FilePath => {
            get_path_hint(parts[1], cwd, fs, mode.dirs_only())
        }
        CompletionMode::None => None,
    }
}

/// Complete command name.
fn complete_command(partial: &str) -> AutocompleteResult {
    let partial_lower = partial.to_lowercase();
    let matches: Vec<String> = Command::names()
        .iter()
        .filter(|cmd| cmd.starts_with(&partial_lower))
        .map(|s| s.to_string())
        .collect();

    match matches.len() {
        0 => AutocompleteResult::None,
        1 => AutocompleteResult::Single(format!("{} ", matches[0])),
        _ => {
            let common = find_common_prefix(&matches);
            AutocompleteResult::Multiple(common, matches)
        }
    }
}

/// Get hint for command name completion.
fn get_command_hint(partial: &str) -> Option<String> {
    let partial_lower = partial.to_lowercase();
    Command::names()
        .iter()
        .find(|cmd| cmd.starts_with(&partial_lower) && **cmd != partial_lower)
        .map(|cmd| cmd[partial.len()..].to_string())
}

/// Complete `sync` subcommands.
///
/// `tail` is everything after `sync ` — e.g. `""`, `"s"`, `"auth "`, `"auth s"`,
/// `"commit my message"`. Returns completions for the first subcommand level
/// (`status`/`commit`/`refresh`/`auth`), and — when the first token is `auth`
/// and there is a trailing space — for the second level (`set`/`clear`).
/// Free-text arguments (after `commit` or `auth set`) receive no completion.
fn complete_sync(tail: &str) -> AutocompleteResult {
    // Split once on the first space to detect the two-level `sync auth ...`
    // grammar. If the tail has no space, we're still completing the first
    // subcommand name.
    match tail.split_once(' ') {
        None => suggest_subcommand("sync", tail, SYNC_SUBCOMMANDS),
        Some(("auth", sub_tail)) => match sub_tail.split_once(' ') {
            // `sync auth <partial>` with no further space
            None => suggest_subcommand("sync auth", sub_tail, SYNC_AUTH_SUBCOMMANDS),
            // `sync auth set <opaque token>` — no completion
            Some(_) => AutocompleteResult::None,
        },
        // `sync commit <message>` / `sync status <junk>` / etc — no completion.
        Some(_) => AutocompleteResult::None,
    }
}

/// Return matches for a subcommand partial against the given list.
fn suggest_subcommand(prefix: &str, partial: &str, options: &[&str]) -> AutocompleteResult {
    let partial_lower = partial.to_lowercase();
    let matches: Vec<String> = options
        .iter()
        .filter(|opt| opt.starts_with(&partial_lower))
        .map(|s| s.to_string())
        .collect();

    match matches.len() {
        0 => AutocompleteResult::None,
        1 => AutocompleteResult::Single(format!("{} {} ", prefix, matches[0])),
        _ => {
            let common = find_common_prefix(&matches);
            AutocompleteResult::Multiple(format!("{} {}", prefix, common), matches)
        }
    }
}

/// Ghost-text hint for `sync` subcommands.
fn get_sync_hint(tail: &str) -> Option<String> {
    match tail.split_once(' ') {
        None => subcommand_hint(tail, SYNC_SUBCOMMANDS),
        Some(("auth", sub_tail)) => match sub_tail.split_once(' ') {
            None => subcommand_hint(sub_tail, SYNC_AUTH_SUBCOMMANDS),
            Some(_) => None,
        },
        Some(_) => None,
    }
}

/// Return the first subcommand that extends the partial, as a suffix hint.
fn subcommand_hint(partial: &str, options: &[&str]) -> Option<String> {
    let partial_lower = partial.to_lowercase();
    options
        .iter()
        .find(|opt| opt.starts_with(&partial_lower) && **opt != partial_lower)
        .map(|opt| opt[partial.len()..].to_string())
}

/// Complete file/directory path.
fn complete_path(
    cmd: &str,
    partial: &str,
    cwd: &VirtualPath,
    fs: &GlobalFs,
    dirs_only: bool,
) -> AutocompleteResult {
    let Some(parsed) = ParsedPath::parse(partial, cwd, fs) else {
        return AutocompleteResult::None;
    };

    let Some(entries) = fs.list_dir(&parsed.search_dir) else {
        return AutocompleteResult::None;
    };

    let matches = get_matching_entries(&entries, parsed.name_part, dirs_only);
    build_path_result(cmd, &parsed, matches)
}

/// Get hint for path completion.
fn get_path_hint(
    partial: &str,
    cwd: &VirtualPath,
    fs: &GlobalFs,
    dirs_only: bool,
) -> Option<String> {
    let parsed = ParsedPath::parse(partial, cwd, fs)?;
    let entries = fs.list_dir(&parsed.search_dir)?;
    let matches = get_matching_entries(&entries, parsed.name_part, dirs_only);

    // Find first match that extends current input
    let name_lower = parsed.name_part.to_lowercase();
    matches
        .iter()
        .find(|(name, _)| name.to_lowercase() != name_lower)
        .map(|(name, is_dir)| {
            let suffix = if *is_dir { "/" } else { "" };
            format!("{}{}", &name[parsed.name_part.len()..], suffix)
        })
}

/// Get filtered entries matching the partial name.
fn get_matching_entries<'a>(
    entries: &'a [DirEntry],
    name_part: &str,
    dirs_only: bool,
) -> Vec<(&'a String, bool)> {
    let name_lower = name_part.to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            if dirs_only && !entry.is_dir {
                return false;
            }
            entry.name.to_lowercase().starts_with(&name_lower)
        })
        .map(|entry| (&entry.name, entry.is_dir))
        .collect()
}

/// Build the autocomplete result from matched paths.
fn build_path_result(
    cmd: &str,
    parsed: &ParsedPath,
    matches: Vec<(&String, bool)>,
) -> AutocompleteResult {
    // Build full paths with directory info
    let full_matches: Vec<(String, bool)> = matches
        .iter()
        .map(|(name, is_dir)| {
            let full_path = format!("{}{}", parsed.dir_part, name);
            (full_path, *is_dir)
        })
        .collect();

    match full_matches.len() {
        0 => AutocompleteResult::None,
        1 => {
            let (path, is_dir) = &full_matches[0];
            let suffix = if *is_dir { "/" } else { " " };
            AutocompleteResult::Single(format!("{} {}{}", cmd, path, suffix))
        }
        _ => {
            let paths: Vec<String> = full_matches.iter().map(|(p, _)| p.clone()).collect();
            let common = find_common_prefix(&paths);

            let display_names: Vec<String> = full_matches
                .iter()
                .map(|(path, is_dir)| {
                    let name = path.rsplit('/').next().unwrap_or(path);
                    if *is_dir {
                        format!("{}/", name)
                    } else {
                        name.to_string()
                    }
                })
                .collect();

            let common_with_cmd = format!("{} {}", cmd, common);
            AutocompleteResult::Multiple(common_with_cmd, display_names)
        }
    }
}

/// Find the common prefix of multiple strings (case-insensitive).
///
/// Operates on Unicode codepoints (chars), not bytes — safe for multi-byte UTF-8.
fn find_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    if strings.len() == 1 {
        return strings[0].clone();
    }

    let first = &strings[0];
    let mut prefix_chars = first.chars().count();

    for s in &strings[1..] {
        let matching = first
            .chars()
            .zip(s.chars())
            .take(prefix_chars)
            .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
            .count();
        prefix_chars = matching;
        if prefix_chars == 0 {
            break;
        }
    }

    first.chars().take(prefix_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_completion_single() {
        match complete_command("cle") {
            AutocompleteResult::Single(s) => assert_eq!(s, "clear "),
            _ => panic!("Expected single match"),
        }
    }

    #[test]
    fn test_command_completion_multiple() {
        match complete_command("c") {
            AutocompleteResult::Multiple(common, matches) => {
                assert_eq!(common, "c");
                assert!(matches.contains(&"cat".to_string()));
                assert!(matches.contains(&"cd".to_string()));
                assert!(matches.contains(&"clear".to_string()));
            }
            _ => panic!("Expected multiple matches"),
        }
    }

    #[test]
    fn test_no_match() {
        assert_eq!(complete_command("xyz"), AutocompleteResult::None);
    }

    #[test]
    fn test_common_prefix() {
        let strings = vec![
            "hello".to_string(),
            "help".to_string(),
            "helicopter".to_string(),
        ];
        assert_eq!(find_common_prefix(&strings), "hel");
    }

    #[test]
    fn test_common_prefix_multibyte() {
        // Korean characters (3 bytes each in UTF-8)
        let strings = vec!["한국어".to_string(), "한국인".to_string()];
        assert_eq!(find_common_prefix(&strings), "한국");
    }

    #[test]
    fn test_common_prefix_emoji() {
        // Emoji (4-byte sequences)
        let strings = vec!["café_1".to_string(), "café_2".to_string()];
        assert_eq!(find_common_prefix(&strings), "café_");
    }

    #[test]
    fn test_common_prefix_mixed_ascii_multibyte() {
        let strings = vec!["abc한".to_string(), "abc中".to_string()];
        assert_eq!(find_common_prefix(&strings), "abc");
    }

    #[test]
    fn test_common_prefix_no_common() {
        let strings = vec!["한".to_string(), "中".to_string()];
        assert_eq!(find_common_prefix(&strings), "");
    }

    #[test]
    fn test_completion_mode() {
        let (mode, _) = CompletionMode::from_input("cd");
        assert_eq!(mode, CompletionMode::Command);

        let (mode, _) = CompletionMode::from_input("cd some/path");
        assert_eq!(mode, CompletionMode::DirectoryPath);

        let (mode, _) = CompletionMode::from_input("cat file.txt");
        assert_eq!(mode, CompletionMode::FilePath);

        let (mode, _) = CompletionMode::from_input("whoami arg");
        assert_eq!(mode, CompletionMode::None);
    }

    #[test]
    fn test_completion_mode_less_no_longer_file() {
        // less is not an implemented command; it should not trigger file-path completion
        let (mode, _) = CompletionMode::from_input("less file.txt");
        assert_eq!(mode, CompletionMode::None);
    }

    #[test]
    fn test_completion_mode_more_no_longer_file() {
        let (mode, _) = CompletionMode::from_input("more file.txt");
        assert_eq!(mode, CompletionMode::None);
    }

    /// Build a small fixture FS with two files and two dirs at `/`:
    /// - `home/` (dir), `help/` (dir)
    /// - `hello.md` (file), `hero.md` (file)
    ///
    /// These names all share the prefix `h`, so a `/h`-style partial
    /// exercises both the dir-only and file+dir classification paths.
    fn write_cmd_fixture() -> GlobalFs {
        use crate::domain::{EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION};
        use crate::engine::filesystem::GlobalFs;
        use crate::ports::{ScannedDirectory, ScannedFile, ScannedSubtree};
        fn file_meta() -> NodeMetadata {
            NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Page,
                bundle: None,
                authored: Fields::default(),
                derived: Fields::default(),
            }
        }
        fn directory_meta(title: &str) -> NodeMetadata {
            NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Directory,
                bundle: None,
                authored: Fields {
                    title: Some(title.to_string()),
                    ..Fields::default()
                },
                derived: Fields::default(),
            }
        }
        let snapshot = ScannedSubtree {
            files: vec![
                ScannedFile {
                    path: "hello.md".to_string(),
                    meta: file_meta(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "hero.md".to_string(),
                    meta: file_meta(),
                    extensions: EntryExtensions::default(),
                },
                // Give the dirs a file each so they exist as directories.
                ScannedFile {
                    path: "home/readme.md".to_string(),
                    meta: file_meta(),
                    extensions: EntryExtensions::default(),
                },
                ScannedFile {
                    path: "help/readme.md".to_string(),
                    meta: file_meta(),
                    extensions: EntryExtensions::default(),
                },
            ],
            directories: vec![
                ScannedDirectory {
                    path: "home".to_string(),
                    meta: directory_meta("Home"),
                },
                ScannedDirectory {
                    path: "help".to_string(),
                    meta: directory_meta("Help"),
                },
            ],
        };
        let mut fs = GlobalFs::empty();
        fs.mount_scanned_subtree(VirtualPath::root(), &snapshot)
            .unwrap();
        fs
    }

    /// Collect the display names from any AutocompleteResult for easier
    /// assertion — works for both Single and Multiple.
    fn matches_set(result: &AutocompleteResult) -> Vec<String> {
        match result {
            AutocompleteResult::Multiple(_, names) => names.clone(),
            AutocompleteResult::Single(s) => vec![s.clone()],
            AutocompleteResult::None => vec![],
        }
    }

    #[test]
    fn test_touch_completes_files_and_dirs() {
        let fs = write_cmd_fixture();
        let result = complete_path(
            "touch",
            "h",
            &VirtualPath::root(),
            &fs,
            /* dirs_only */ false,
        );
        let names = matches_set(&result);
        // Should include both files and dirs.
        assert!(
            names.iter().any(|n| n == "hello.md"),
            "touch should surface files; got {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n == "hero.md"),
            "touch should surface files; got {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n == "home/"),
            "touch should surface dirs; got {:?}",
            names
        );
        assert!(
            names.iter().any(|n| n == "help/"),
            "touch should surface dirs; got {:?}",
            names
        );
    }

    #[test]
    fn test_rm_completes_files_and_dirs() {
        let fs = write_cmd_fixture();
        let result = complete_path(
            "rm",
            "h",
            &VirtualPath::root(),
            &fs,
            /* dirs_only */ false,
        );
        let names = matches_set(&result);
        assert!(names.iter().any(|n| n == "hello.md"), "got {:?}", names);
        assert!(names.iter().any(|n| n == "home/"), "got {:?}", names);
    }

    #[test]
    fn test_edit_completes_files_and_dirs() {
        let fs = write_cmd_fixture();
        let result = complete_path(
            "edit",
            "h",
            &VirtualPath::root(),
            &fs,
            /* dirs_only */ false,
        );
        let names = matches_set(&result);
        assert!(names.iter().any(|n| n == "hello.md"), "got {:?}", names);
        assert!(names.iter().any(|n| n == "home/"), "got {:?}", names);
    }

    #[test]
    fn test_mkdir_completes_dirs_only() {
        let fs = write_cmd_fixture();
        let result = complete_path(
            "mkdir",
            "h",
            &VirtualPath::root(),
            &fs,
            /* dirs_only */ true,
        );
        let names = matches_set(&result);
        // Dirs yes, files no.
        assert!(names.iter().any(|n| n == "home/"), "got {:?}", names);
        assert!(names.iter().any(|n| n == "help/"), "got {:?}", names);
        assert!(
            !names.iter().any(|n| n == "hello.md"),
            "mkdir must NOT surface files; got {:?}",
            names
        );
        assert!(
            !names.iter().any(|n| n == "hero.md"),
            "mkdir must NOT surface files; got {:?}",
            names
        );
    }

    #[test]
    fn test_rmdir_completes_dirs_only() {
        let fs = write_cmd_fixture();
        let result = complete_path(
            "rmdir",
            "h",
            &VirtualPath::root(),
            &fs,
            /* dirs_only */ true,
        );
        let names = matches_set(&result);
        assert!(names.iter().any(|n| n == "home/"), "got {:?}", names);
        assert!(
            !names.iter().any(|n| n == "hello.md"),
            "rmdir must NOT surface files; got {:?}",
            names
        );
    }

    #[test]
    fn test_classification_write_commands() {
        let (mode, _) = CompletionMode::from_input("touch foo");
        assert_eq!(mode, CompletionMode::FilePath);
        let (mode, _) = CompletionMode::from_input("rm foo");
        assert_eq!(mode, CompletionMode::FilePath);
        let (mode, _) = CompletionMode::from_input("edit foo");
        assert_eq!(mode, CompletionMode::FilePath);
        let (mode, _) = CompletionMode::from_input("mkdir foo");
        assert_eq!(mode, CompletionMode::DirectoryPath);
        let (mode, _) = CompletionMode::from_input("rmdir foo");
        assert_eq!(mode, CompletionMode::DirectoryPath);
    }

    #[test]
    fn test_sync_empty_suggests_all_subcommands() {
        let result = complete_sync("");
        match result {
            AutocompleteResult::Multiple(_, names) => {
                for expected in &["status", "commit", "refresh", "auth"] {
                    assert!(
                        names.iter().any(|n| n == expected),
                        "missing {}: got {:?}",
                        expected,
                        names
                    );
                }
            }
            other => panic!("expected Multiple, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_s_suggests_status() {
        let result = complete_sync("s");
        match result {
            AutocompleteResult::Single(s) => assert_eq!(s, "sync status "),
            other => panic!("expected Single(\"sync status \"), got {:?}", other),
        }
    }

    #[test]
    fn test_sync_auth_suggests_set_and_clear() {
        let result = complete_sync("auth ");
        match result {
            AutocompleteResult::Multiple(_, names) => {
                assert!(names.iter().any(|n| n == "set"), "got {:?}", names);
                assert!(names.iter().any(|n| n == "clear"), "got {:?}", names);
                assert_eq!(names.len(), 2);
            }
            other => panic!("expected Multiple, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_auth_s_suggests_set() {
        let result = complete_sync("auth s");
        match result {
            AutocompleteResult::Single(s) => assert_eq!(s, "sync auth set "),
            other => panic!("expected Single(\"sync auth set \"), got {:?}", other),
        }
    }

    #[test]
    fn test_sync_commit_no_completion() {
        // `sync commit ` (empty message body) → no suggestions.
        assert_eq!(complete_sync("commit "), AutocompleteResult::None);
        // Mid-message — also nothing.
        assert_eq!(complete_sync("commit fixing the"), AutocompleteResult::None);
    }

    #[test]
    fn test_sync_auth_set_no_completion() {
        // Opaque token after `set` — no suggestions.
        assert_eq!(complete_sync("auth set "), AutocompleteResult::None);
        assert_eq!(complete_sync("auth set ghp_"), AutocompleteResult::None);
    }

    #[test]
    fn test_sync_routes_through_autocomplete() {
        // Sanity check that the top-level `autocomplete()` dispatcher
        // routes `sync ...` to `complete_sync`, not to the generic
        // mode-based branches. An empty GlobalFs is fine because `complete_sync`
        // never touches the filesystem.
        let fs = GlobalFs::empty();
        let cwd = VirtualPath::root();
        let result = autocomplete("sync s", &cwd, &fs);
        match result {
            AutocompleteResult::Single(s) => assert_eq!(s, "sync status "),
            other => panic!("expected Single, got {:?}", other),
        }
    }

    #[test]
    fn test_sync_hint_extends_partial() {
        assert_eq!(get_sync_hint("s"), Some("tatus".to_string()));
        assert_eq!(get_sync_hint("auth c"), Some("lear".to_string()));
        // Already complete — no hint.
        assert_eq!(get_sync_hint("status"), None);
        // Free-text regions don't get hints.
        assert_eq!(get_sync_hint("commit message"), None);
        assert_eq!(get_sync_hint("auth set token"), None);
    }
}
