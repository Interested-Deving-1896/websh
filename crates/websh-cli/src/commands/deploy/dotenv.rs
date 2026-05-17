use std::fs;
use std::path::Path;

use anyhow::Context;

use crate::CliResult;

pub(super) fn load_dotenv(root: &Path) -> CliResult<Vec<(String, String)>> {
    let path = root.join(".env");
    if !path.exists() {
        return Ok(Vec::new());
    }

    let body = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(parse_dotenv(&body))
}

fn parse_dotenv(body: &str) -> Vec<(String, String)> {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }

            Some((key.to_string(), unquote_env_value(value.trim()).to_string()))
        })
        .collect()
}

fn unquote_env_value(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv;

    #[test]
    fn parses_dotenv_values_for_child_processes() {
        let envs = parse_dotenv(
            r#"
            # comment
            PINATA_JWT="secret"
            export PINATA_GROUP=websh
            "#,
        );
        assert_eq!(
            envs,
            vec![
                ("PINATA_JWT".to_string(), "secret".to_string()),
                ("PINATA_GROUP".to_string(), "websh".to_string())
            ]
        );
    }
}
