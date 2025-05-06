use std::{fmt::Debug, str::FromStr};

use async_trait::async_trait;
use light_compressed_account::compressed_account::{
    CompressedAccount, CompressedAccountData, CompressedAccountWithMerkleContext, MerkleContext,
};
use light_concurrent_merkle_tree::light_hasher::Poseidon;
use light_indexed_merkle_tree::{
    array::{IndexedArray, IndexedElement, IndexedElementBundle},
    reference::IndexedMerkleTree,
};
use light_merkle_tree_metadata::QueueType;
use light_merkle_tree_reference::MerkleTree;
use light_prover_client::non_inclusion::merkle_non_inclusion_proof_inputs::{
    get_non_inclusion_proof_inputs, NonInclusionMerkleProofInputs,
};
use light_sdk::token::{AccountState, TokenData, TokenDataWithMerkleContext};
use num_bigint::{BigInt, BigUint};
use num_traits::ops::bytes::FromBytes;
use photon_api::models::{
    Account, CompressedProofWithContext, CompressedProofWithContextV2, TokenAccount,
    TokenAccountList, TokenBalanceList,
};
use solana_pubkey::Pubkey;
use types::MerkleProof;

use light_client::{
    fee::FeeConfig,
    rpc::{
        types::{ProofRpcResult, ProofRpcResultV2},
        RpcConnection,
    },
};

#[derive(Debug, Clone)]
pub struct StateMerkleTreeBundle {
    pub rollover_fee: i64,
    pub merkle_tree: Box<MerkleTree<Poseidon>>,
    pub accounts: StateMerkleTreeAccounts,
    pub version: u64,
    pub output_queue_elements: Vec<([u8; 32], u64)>,
    pub input_leaf_indices: Vec<LeafIndexInfo>,
}
