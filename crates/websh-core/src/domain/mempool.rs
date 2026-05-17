//! Mempool-domain metadata — sibling to `NodeMetadata` in manifest entries.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MempoolStatus {
    Draft,
    Review,
}

impl FromStr for MempoolStatus {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, ()> {
        match value {
            "draft" => Ok(Self::Draft),
            "review" => Ok(Self::Review),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Med,
    High,
}

impl FromStr for Priority {
    type Err = ();
    fn from_str(value: &str) -> Result<Self, ()> {
        match value {
            "low" => Ok(Self::Low),
            "med" => Ok(Self::Med),
            "high" => Ok(Self::High),
            _ => Err(()),
        }
    }
}

/// Mempool-only metadata block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MempoolFields {
    pub status: MempoolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}
