use borsh::{BorshDeserialize, BorshSerialize};
use light_compressed_account::instruction_data::compressed_proof::CompressedProof;
use solana_pubkey::Pubkey;

use super::IndexerError;

pub struct ProofOfLeaf {
    pub leaf: [u8; 32],
    pub proof: Vec<[u8; 32]>,
}

pub type Address = [u8; 32];
pub type Hash = [u8; 32];

pub struct AddressWithTree {
    pub address: Address,
    pub tree: Pubkey,
}

#[derive(Debug, Clone)]
pub struct MerkleProofWithContext {
    pub proof: Vec<[u8; 32]>,
    pub root: [u8; 32],
    pub leaf_index: u64,
    pub leaf: [u8; 32],
    pub merkle_tree: [u8; 32],
    pub root_seq: u64,
    pub tx_hash: Option<[u8; 32]>,
    pub account_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct MerkleProof {
    pub hash: String,
    pub leaf_index: u64,
    pub merkle_tree: String,
    pub proof: Vec<[u8; 32]>,
    pub root_seq: u64,
    pub root: [u8; 32],
}

#[derive(Debug, Clone, BorshDeserialize, BorshSerialize)]
pub struct ProofRpcResult {
    pub proof: CompressedProof,
    pub root_indices: Vec<u16>,
    pub address_root_indices: Vec<u16>,
}

impl TryFrom<photon_api::models::CompressedProofWithContext> for ProofRpcResult {
    type Error = IndexerError;

    fn try_from(
        value: photon_api::models::CompressedProofWithContext,
    ) -> Result<Self, Self::Error> {
        let mut proof = CompressedProof::default();
        proof.a = value
            .compressed_proof
            .a
            .try_into()
            .map_err(|_| IndexerError::InvalidResponseData)?;
        proof.b = value
            .compressed_proof
            .b
            .try_into()
            .map_err(|_| IndexerError::InvalidResponseData)?;
        proof.c = value
            .compressed_proof
            .c
            .try_into()
            .map_err(|_| IndexerError::InvalidResponseData)?;
        Ok(Self {
            proof,
            root_indices: value.root_indices[..value.leaf_indices.len()]
                .iter()
                .map(|x| {
                    (*x).try_into()
                        .map_err(|_| IndexerError::InvalidResponseData)
                })
                .collect::<Result<Vec<u16>, _>>()?,
            address_root_indices: value.root_indices[value.leaf_indices.len()..]
                .iter()
                .map(|x| {
                    (*x).try_into()
                        .map_err(|_| IndexerError::InvalidResponseData)
                })
                .collect::<Result<Vec<u16>, _>>()?,
        })
    }
}

#[cfg(feature = "v2")]
#[derive(Debug, Default)]
pub struct ProofRpcResultV2 {
    pub proof: Option<CompressedProof>,
    // If none -> proof by index  and not included in zkp, else included in zkp
    pub root_indices: Vec<Option<u16>>,
    pub address_root_indices: Vec<u16>,
}
#[cfg(feature = "v2")]
impl TryFrom<photon_api::models::CompressedProofWithContextV2> for ProofRpcResultV2 {
    type Error = IndexerError;

    fn try_from(
        value: photon_api::models::CompressedProofWithContextV2,
    ) -> Result<Self, IndexerError> {
        let proof = if let Some(value) = value.compressed_proof {
            let mut proof = CompressedProof::default();
            proof.a = value
                .a
                .try_into()
                .map_err(|_| IndexerError::InvalidResponseData)?;
            proof.b = value
                .b
                .try_into()
                .map_err(|_| IndexerError::InvalidResponseData)?;
            proof.c = value
                .c
                .try_into()
                .map_err(|_| IndexerError::InvalidResponseData)?;
            Some(proof)
        } else {
            None
        };

        Ok(Self {
            proof,
            root_indices: value.root_indices[..value.leaf_indices.len()]
                .iter()
                .map(|x| {
                    if x.prove_by_index {
                        None
                    } else {
                        Some(x.root_index)
                    }
                })
                .collect::<Vec<Option<u16>>>(),
            address_root_indices: value.root_indices[value.leaf_indices.len()..]
                .iter()
                .map(|x| x.root_index)
                .collect::<Vec<u16>>(),
        })
    }
}
