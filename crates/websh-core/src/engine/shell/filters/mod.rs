//! Pipe filter commands (grep, head, tail, wc).
//!
//! These filters operate on output lines from other commands,
//! enabling Unix-style piping: `ls | grep foo | head -5`

use crate::engine::shell::config::pipe_filters;

use super::{CommandResult, OutputLine, OutputLineData};

/// Apply a filter command to output lines.
pub fn apply_filter(cmd: &str, args: &[String], lines: Vec<OutputLine>) -> CommandResult {
    match cmd.to_lowercase().as_str() {
        "grep" => filter_grep(args, lines),
        "head" => filter_head(args, lines),
        "tail" => filter_tail(args, lines),
        "wc" => filter_wc(lines),
        _ => CommandResult::error_line(format!(
            "Pipe: unknown filter '{}'. Supported: grep, head, tail, wc",
            cmd
        ))
        .with_exit_code(127),
    }
}

fn filter_grep(args: &[String], lines: Vec<OutputLine>) -> CommandResult {
    // Parse flags and pattern.
    let mut ignore_case = false;
    let mut invert = false;
    let mut fixed_strings = false;
    let mut pattern: Option<&str> = None;

    for arg in args {
        if arg.starts_with("--") {
            match arg.as_str() {
                "--ignore-case" => ignore_case = true,
                "--invert-match" => invert = true,
                "--extended-regexp" => {} // no-op: regex crate is always extended
                "--fixed-strings" => fixed_strings = true,
                _ => {
                    return CommandResult::error_line(format!("grep: unknown option: {}", arg))
                        .with_exit_code(2);
                }
            }
        } else if let Some(rest) = arg.strip_prefix('-') {
            if rest.is_empty() {
                // A bare "-" is not a flag; treat as pattern if pattern is None.
                if pattern.is_none() {
                    pattern = Some(arg.as_str());
                } else {
                    return CommandResult::error_line(
                        "grep: extra argument (multiple patterns or file args are not supported)"
                            .to_string(),
                    )
                    .with_exit_code(2);
                }
            } else {
                for ch in rest.chars() {
                    match ch {
                        'i' => ignore_case = true,
                        'v' => invert = true,
                        'E' => {} // no-op
                        'F' => fixed_strings = true,
                        other => {
                            return CommandResult::error_line(format!(
                                "grep: unknown option: -{}",
                                other
                            ))
                            .with_exit_code(2);
                        }
                    }
                }
            }
        } else if pattern.is_none() {
            pattern = Some(arg.as_str());
        } else {
            // extra positional arg: not supported
            return CommandResult::error_line(
                "grep: extra argument (multiple patterns or file args are not supported)"
                    .to_string(),
            )
            .with_exit_code(2);
        }
    }

    let Some(pat) = pattern else {
        return CommandResult::error_line("grep: missing pattern").with_exit_code(2);
    };

    // With -F, escape regex metacharacters so the pattern matches literally.
    let effective_pattern = if fixed_strings {
        regex::escape(pat)
    } else {
        pat.to_string()
    };

    // Compile regex (with case-insensitive flag if requested).
    let regex = match build_grep_regex(&effective_pattern, ignore_case) {
        Ok(r) => r,
        Err(e) => {
            return CommandResult::error_line(format!("grep: invalid regex: {}", e))
                .with_exit_code(2);
        }
    };

    let matched: Vec<OutputLine> = lines
        .into_iter()
        .filter(|line| {
            let is_match = regex_matches_line(&regex, &line.data);
            is_match ^ invert
        })
        .collect();

    let exit_code = if matched.is_empty() { 1 } else { 0 };
    CommandResult::output(matched).with_exit_code(exit_code)
}

fn build_grep_regex(pattern: &str, ignore_case: bool) -> Result<regex::Regex, regex::Error> {
    let mut builder = regex::RegexBuilder::new(pattern);
    builder.case_insensitive(ignore_case);
    if ignore_case {
        builder.unicode(false);
    }
    builder.build()
}

fn regex_matches_line(re: &regex::Regex, data: &OutputLineData) -> bool {
    match data {
        OutputLineData::Text(s)
        | OutputLineData::Error(s)
        | OutputLineData::Success(s)
        | OutputLineData::Info(s)
        | OutputLineData::Ascii(s) => re.is_match(s),
        OutputLineData::ListEntry { name, .. } => re.is_match(name),
        OutputLineData::Command { input, .. } => re.is_match(input),
        OutputLineData::Empty => false,
    }
}

fn filter_head(args: &[String], lines: Vec<OutputLine>) -> CommandResult {
    let n = match parse_count(args, pipe_filters::DEFAULT_HEAD_LINES) {
        Ok(n) => n,
        Err(msg) => {
            return CommandResult::error_line(format!("head: {}", msg)).with_exit_code(2);
        }
    };
    CommandResult::output(lines.into_iter().take(n).collect())
}

fn filter_tail(args: &[String], lines: Vec<OutputLine>) -> CommandResult {
    let n = match parse_count(args, pipe_filters::DEFAULT_TAIL_LINES) {
        Ok(n) => n,
        Err(msg) => {
            return CommandResult::error_line(format!("tail: {}", msg)).with_exit_code(2);
        }
    };
    let len = lines.len();
    CommandResult::output(lines.into_iter().skip(len.saturating_sub(n)).collect())
}

fn filter_wc(lines: Vec<OutputLine>) -> CommandResult {
    let count = lines
        .iter()
        .filter(|l| !matches!(l.data, OutputLineData::Empty))
        .count();
    CommandResult::output(vec![OutputLine::text(format!("{}", count))])
}

/// Parse the count argument for head/tail.
///
/// Supports:
/// - No args: returns `default`.
/// - `-N` where N is a non-negative integer (e.g., `-5`).
/// - `-n N` where N is a non-negative integer (e.g., `-n 5`).
///
/// Rejects:
/// - `--N`, `---N`, etc.
/// - Non-numeric: `-abc`, `abc`.
/// - Unknown flags.
fn parse_count(args: &[String], default: usize) -> Result<usize, String> {
    match args.len() {
        0 => Ok(default),
        1 => {
            let arg = &args[0];
            // `-n` alone is incomplete; fallthrough to error below
            if arg == "-n" {
                return Err("option requires an argument: -n".to_string());
            }
            // Bulk reject any double-dash prefix
            if arg.starts_with("--") {
                return Err(format!("unknown option: {}", arg));
            }
            if let Some(rest) = arg.strip_prefix('-') {
                // must be `-N` where N is integer
                rest.parse::<usize>()
                    .map_err(|_| format!("invalid option: -{}", rest))
            } else {
                // bare positional like "5" is not POSIX but also not accepted
                Err(format!("unexpected argument: {}", arg))
            }
        }
        2 => {
            if args[0] == "-n" {
                args[1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid number: {}", args[1]))
            } else {
                Err(format!("unknown options: {} {}", args[0], args[1]))
            }
        }
        _ => Err("too many arguments".to_string()),
    }
}

#[cfg(test)]
mod tests;
