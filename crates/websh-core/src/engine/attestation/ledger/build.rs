use crate::engine::attestation::artifact::{CONTENT_HASH, sha256_hex};

use super::{
    CONTENT_LEDGER_GENESIS_HASH, CONTENT_LEDGER_SCHEME, ContentLedger, ContentLedgerBlock,
    ContentLedgerBlockForHash, ContentLedgerInput, LedgerHashError,
};

impl ContentLedger {
    pub fn new(mut inputs: Vec<ContentLedgerInput>) -> Result<Self, LedgerHashError> {
        inputs.sort_by(|left, right| left.sort_key.cmp(&right.sort_key));

        let genesis_hash = CONTENT_LEDGER_GENESIS_HASH.to_string();
        let mut prev_block_sha256 = genesis_hash.clone();
        let mut blocks = Vec::with_capacity(inputs.len());

        for (index, input) in inputs.into_iter().enumerate() {
            let mut block = ContentLedgerBlock {
                height: index as u64 + 1,
                sort_key: input.sort_key,
                prev_block_sha256,
                block_sha256: String::new(),
                entry: input.entry,
            };
            block.block_sha256 = compute_block_sha256(&block)?;
            prev_block_sha256 = block.block_sha256.clone();
            blocks.push(block);
        }

        let block_count = blocks.len();
        let chain_head = blocks
            .last()
            .map(|block| block.block_sha256.clone())
            .unwrap_or_else(|| genesis_hash.clone());

        Ok(Self {
            version: 1,
            scheme: CONTENT_LEDGER_SCHEME.to_string(),
            hash: CONTENT_HASH.to_string(),
            genesis_hash,
            blocks,
            block_count,
            chain_head,
        })
    }
}

pub fn compute_block_sha256(block: &ContentLedgerBlock) -> Result<String, LedgerHashError> {
    serde_json::to_vec(&ContentLedgerBlockForHash {
        height: block.height,
        sort_key: &block.sort_key,
        prev_block_sha256: &block.prev_block_sha256,
        entry: &block.entry,
    })
    .map(|bytes| sha256_hex(&bytes))
    .map_err(|source| LedgerHashError::Block { source })
}
