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
use solana_sdk::pubkey::Pubkey;

use crate::{
    rpc::{types::ProofRpcResult, RpcConnection},
    transaction_params::FeeConfig,
};

pub mod photon_indexer;

mod base58;
mod error;
mod types;

pub use base58::Base58Conversions;
pub use error::IndexerError;
pub use types::{Address, AddressWithTree, Hash, MerkleProofWithContext, ProofOfLeaf};

#[derive(Debug, Clone)]
pub struct AddressQueueIndex {
    pub address: [u8; 32],
    pub queue_index: u64,
}

#[derive(Debug, Clone)]
pub struct BatchAddressUpdateIndexerResponse {
    pub batch_start_index: u64,
    pub addresses: Vec<AddressQueueIndex>,
    pub non_inclusion_proofs: Vec<NewAddressProofWithContext<40>>,
    pub subtrees: Vec<[u8; 32]>,
}

#[async_trait]
pub trait Indexer<R: RpcConnection>: Sync + Send + Debug + 'static {
    /// Returns queue elements from the queue with the given merkle tree pubkey. For input
    /// queues account compression program does not store queue elements in the
    /// account data but only emits these in the public transaction event. The
    /// indexer needs the queue elements to create batch update proofs.
    async fn get_queue_elements(
        &mut self,
        merkle_tree_pubkey: [u8; 32],
        queue_type: QueueType,
        num_elements: u16,
        start_offset: Option<u64>,
    ) -> Result<Vec<MerkleProofWithContext>, IndexerError>;

    async fn get_subtrees(
        &self,
        merkle_tree_pubkey: [u8; 32],
    ) -> Result<Vec<[u8; 32]>, IndexerError>;

    async fn create_proof_for_compressed_accounts(
        &mut self,
        compressed_accounts: Option<Vec<[u8; 32]>>,
        state_merkle_tree_pubkeys: Option<Vec<Pubkey>>,
        new_addresses: Option<&[[u8; 32]]>,
        address_merkle_tree_pubkeys: Option<Vec<Pubkey>>,
        rpc: &mut R,
    ) -> Result<ProofRpcResult, IndexerError>;

    async fn get_multiple_compressed_account_proofs(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<MerkleProof>, IndexerError>;

    async fn get_compressed_accounts_by_owner_v2(
        &self,
        owner: &Pubkey,
    ) -> Result<Vec<CompressedAccountWithMerkleContext>, IndexerError>;

    async fn get_compressed_token_accounts_by_owner_v2(
        &self,
        owner: &Pubkey,
        mint: Option<Pubkey>,
    ) -> Result<Vec<TokenDataWithMerkleContext>, IndexerError>;

    async fn get_compressed_account(
        &self,
        address: Option<Address>,
        hash: Option<Hash>,
    ) -> Result<Account, IndexerError>;

    async fn get_compressed_token_accounts_by_owner(
        &self,
        owner: &Pubkey,
        mint: Option<Pubkey>,
    ) -> Result<Vec<TokenDataWithMerkleContext>, IndexerError>;

    async fn get_compressed_account_balance(
        &self,
        address: Option<Address>,
        hash: Option<Hash>,
    ) -> Result<u64, IndexerError>;

    async fn get_compressed_token_account_balance(
        &self,
        address: Option<Address>,
        hash: Option<Hash>,
    ) -> Result<u64, IndexerError>;

    async fn get_multiple_compressed_accounts(
        &self,
        addresses: Option<Vec<Address>>,
        hashes: Option<Vec<Hash>>,
    ) -> Result<Vec<Account>, IndexerError>;

    async fn get_compressed_token_balances_by_owner(
        &self,
        owner: &Pubkey,
        mint: Option<Pubkey>,
    ) -> Result<TokenBalanceList, IndexerError>;

    async fn get_compression_signatures_for_account(
        &self,
        hash: Hash,
    ) -> Result<Vec<String>, IndexerError>;

    async fn get_multiple_new_address_proofs(
        &self,
        merkle_tree_pubkey: [u8; 32],
        addresses: Vec<[u8; 32]>,
    ) -> Result<Vec<NewAddressProofWithContext<16>>, IndexerError>;

    async fn get_multiple_new_address_proofs_h40(
        &self,
        merkle_tree_pubkey: [u8; 32],
        addresses: Vec<[u8; 32]>,
    ) -> Result<Vec<NewAddressProofWithContext<40>>, IndexerError>;

    async fn get_validity_proof(
        &self,
        hashes: Vec<Hash>,
        new_addresses_with_trees: Vec<AddressWithTree>,
    ) -> Result<CompressedProofWithContext, IndexerError>;

    async fn get_validity_proof_v2(
        &self,
        hashes: Vec<Hash>,
        new_addresses_with_trees: Vec<AddressWithTree>,
    ) -> Result<CompressedProofWithContextV2, IndexerError>;

    async fn get_indexer_slot(&self, r: &mut R) -> Result<u64, IndexerError>;

    fn get_address_merkle_trees(&self) -> &Vec<AddressMerkleTreeBundle>;

    async fn get_address_queue_with_proofs(
        &mut self,
        merkle_tree_pubkey: &Pubkey,
        zkp_batch_size: u16,
    ) -> Result<BatchAddressUpdateIndexerResponse, IndexerError>;
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

// For consistency with the Photon API.
#[derive(Clone, Debug, PartialEq)]
pub struct NewAddressProofWithContext<const NET_HEIGHT: usize> {
    pub merkle_tree: [u8; 32],
    pub root: [u8; 32],
    pub root_seq: u64,
    pub low_address_index: u64,
    pub low_address_value: [u8; 32],
    pub low_address_next_index: u64,
    pub low_address_next_value: [u8; 32],
    pub low_address_proof: [[u8; 32]; NET_HEIGHT],
    pub new_low_element: Option<IndexedElement<usize>>,
    pub new_element: Option<IndexedElement<usize>>,
    pub new_element_next_value: Option<BigUint>,
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub struct StateMerkleTreeAccounts {
    pub merkle_tree: Pubkey,
    pub nullifier_queue: Pubkey,
    pub cpi_context: Pubkey,
}

#[derive(Debug, Clone, Copy)]
pub struct AddressMerkleTreeAccounts {
    pub merkle_tree: Pubkey,
    pub queue: Pubkey,
}

#[derive(Debug, Clone)]
pub struct LeafIndexInfo {
    pub leaf_index: u32,
    pub leaf: [u8; 32],
    pub tx_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct StateMerkleTreeBundle {
    pub rollover_fee: i64,
    pub merkle_tree: Box<MerkleTree<Poseidon>>,
    pub accounts: StateMerkleTreeAccounts,
    pub version: u64,
    pub output_queue_elements: Vec<([u8; 32], u64)>,
    pub input_leaf_indices: Vec<LeafIndexInfo>,
}

#[derive(Debug, Clone)]
pub enum IndexedMerkleTreeVersion {
    V1(Box<IndexedMerkleTree<Poseidon, usize>>),
    V2(Box<light_merkle_tree_reference::indexed::IndexedMerkleTree<Poseidon, usize>>),
}

#[derive(Debug, Clone)]
pub struct AddressMerkleTreeBundle {
    pub rollover_fee: i64,
    pub merkle_tree: IndexedMerkleTreeVersion,
    indexed_array: Box<IndexedArray<Poseidon, usize>>,
    pub accounts: AddressMerkleTreeAccounts,
    pub queue_elements: Vec<[u8; 32]>,
}

impl AddressMerkleTreeBundle {
    pub fn new_v1(accounts: AddressMerkleTreeAccounts) -> Result<Self, IndexerError> {
        let height = 26;
        let canopy = 10;
        let mut merkle_tree = Box::new(IndexedMerkleTree::<Poseidon, usize>::new(height, canopy)?);
        merkle_tree.init()?;
        let mut indexed_array = Box::<IndexedArray<Poseidon, usize>>::default();
        indexed_array.init()?;
        Ok(AddressMerkleTreeBundle {
            merkle_tree: IndexedMerkleTreeVersion::V1(merkle_tree),
            indexed_array,
            accounts,
            rollover_fee: FeeConfig::default().address_queue_rollover as i64,
            queue_elements: vec![],
        })
    }

    pub fn new_v2(accounts: AddressMerkleTreeAccounts) -> Result<Self, IndexerError> {
        let height = 40;
        let canopy = 0;
        let merkle_tree = IndexedMerkleTreeVersion::V2(Box::new(
            light_merkle_tree_reference::indexed::IndexedMerkleTree::<Poseidon, usize>::new(
                height, canopy,
            )?,
        ));

        Ok(AddressMerkleTreeBundle {
            merkle_tree,
            indexed_array: Box::default(),
            accounts,
            rollover_fee: FeeConfig::default().address_queue_rollover as i64,
            queue_elements: vec![],
        })
    }

    pub fn get_v1_indexed_merkle_tree(&self) -> Option<&IndexedMerkleTree<Poseidon, usize>> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn get_v1_indexed_merkle_tree_mut(
        &mut self,
    ) -> Option<&mut IndexedMerkleTree<Poseidon, usize>> {
        match &mut self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn get_v2_indexed_merkle_tree(
        &self,
    ) -> Option<&light_merkle_tree_reference::indexed::IndexedMerkleTree<Poseidon, usize>> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V2(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn get_v2_indexed_merkle_tree_mut(
        &mut self,
    ) -> Option<&mut light_merkle_tree_reference::indexed::IndexedMerkleTree<Poseidon, usize>> {
        match &mut self.merkle_tree {
            IndexedMerkleTreeVersion::V2(tree) => Some(tree),
            _ => None,
        }
    }

    pub fn get_subtrees(&self) -> Vec<[u8; 32]> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => tree.merkle_tree.get_subtrees(),
            IndexedMerkleTreeVersion::V2(tree) => tree.merkle_tree.get_subtrees(),
        }
    }

    pub fn root(&self) -> [u8; 32] {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => tree.merkle_tree.root(),
            IndexedMerkleTreeVersion::V2(tree) => tree.merkle_tree.root(),
        }
    }

    pub fn find_low_element_for_nonexistent(
        &self,
        value: &BigUint,
    ) -> Result<(IndexedElement<usize>, BigUint), IndexerError> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(_) => {
                Ok(self.indexed_array.find_low_element_for_nonexistent(value)?)
            }
            IndexedMerkleTreeVersion::V2(tree) => {
                let (indexed_element, next_value) =
                    tree.indexed_array.find_low_element_for_nonexistent(value)?;
                Ok((
                    IndexedElement {
                        index: indexed_element.index,
                        value: indexed_element.value.clone(),
                        next_index: indexed_element.next_index,
                    },
                    next_value,
                ))
            }
        }
    }

    pub fn new_element_with_low_element_index(
        &self,
        index: usize,
        value: &BigUint,
    ) -> Result<IndexedElementBundle<usize>, IndexerError> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(_) => Ok(self
                .indexed_array
                .new_element_with_low_element_index(index, value)?),
            IndexedMerkleTreeVersion::V2(tree) => {
                let res = tree
                    .indexed_array
                    .new_element_with_low_element_index(index, value)?;
                Ok(IndexedElementBundle {
                    new_element: IndexedElement {
                        index: res.new_element.index,
                        value: res.new_element.value.clone(),
                        next_index: res.new_element.next_index,
                    },
                    new_low_element: IndexedElement {
                        index: res.new_low_element.index,
                        value: res.new_low_element.value.clone(),
                        next_index: res.new_low_element.next_index,
                    },
                    new_element_next_value: res.new_element_next_value.clone(),
                })
            }
        }
    }

    pub fn get_proof_of_leaf(
        &self,
        index: usize,
        full: bool,
    ) -> Result<Vec<[u8; 32]>, IndexerError> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => Ok(tree.get_proof_of_leaf(index, full)?.to_vec()),
            IndexedMerkleTreeVersion::V2(tree) => Ok(tree.get_proof_of_leaf(index, full)?),
        }
    }

    pub fn append(&mut self, value: &BigUint) -> Result<(), IndexerError> {
        match &mut self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => {
                tree.append(value, &mut self.indexed_array)?;
                Ok(())
            }
            IndexedMerkleTreeVersion::V2(tree) => {
                tree.append(value)?;
                Ok(())
            }
        }
    }

    pub fn get_non_inclusion_proof_inputs(
        &self,
        value: &[u8; 32],
    ) -> Result<NonInclusionMerkleProofInputs, IndexerError> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => Ok(get_non_inclusion_proof_inputs(
                value,
                tree,
                &self.indexed_array,
            )),
            IndexedMerkleTreeVersion::V2(merkle_tree) => {
                let non_inclusion_proof =
                    merkle_tree.get_non_inclusion_proof(&BigUint::from_be_bytes(value))?;
                let proof = non_inclusion_proof
                    .merkle_proof
                    .iter()
                    .map(|x| BigInt::from_be_bytes(x))
                    .collect();
                Ok(NonInclusionMerkleProofInputs {
                    root: BigInt::from_be_bytes(merkle_tree.root().as_slice()),
                    value: BigInt::from_be_bytes(value),
                    leaf_lower_range_value: BigInt::from_be_bytes(
                        &non_inclusion_proof.leaf_lower_range_value,
                    ),
                    leaf_higher_range_value: BigInt::from_be_bytes(
                        &non_inclusion_proof.leaf_higher_range_value,
                    ),
                    merkle_proof_hashed_indexed_element_leaf: proof,
                    index_hashed_indexed_element_leaf: BigInt::from(non_inclusion_proof.leaf_index),
                    next_index: BigInt::from(non_inclusion_proof.next_index),
                })
            }
        }
    }

    pub fn right_most_index(&self) -> usize {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => tree.merkle_tree.rightmost_index,
            IndexedMerkleTreeVersion::V2(tree) => tree.merkle_tree.rightmost_index,
        }
    }

    pub fn append_with_low_element_index(
        &mut self,
        index: usize,
        value: &BigUint,
    ) -> Result<IndexedElementBundle<usize>, IndexerError> {
        match &mut self.merkle_tree {
            IndexedMerkleTreeVersion::V1(_) => Ok(self
                .indexed_array
                .append_with_low_element_index(index, value)?),
            IndexedMerkleTreeVersion::V2(_) => {
                unimplemented!("append_with_low_element_index")
            }
        }
    }

    pub fn sequence_number(&self) -> u64 {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => tree.merkle_tree.sequence_number as u64,
            IndexedMerkleTreeVersion::V2(tree) => tree.merkle_tree.sequence_number as u64,
        }
    }

    pub fn height(&self) -> usize {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => tree.merkle_tree.height,
            IndexedMerkleTreeVersion::V2(tree) => tree.merkle_tree.height,
        }
    }

    pub fn get_path_of_leaf(
        &self,
        index: usize,
        full: bool,
    ) -> Result<Vec<[u8; 32]>, IndexerError> {
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => Ok(tree.get_path_of_leaf(index, full)?.to_vec()),
            IndexedMerkleTreeVersion::V2(tree) => Ok(tree.get_path_of_leaf(index, full)?),
        }
    }

    pub fn indexed_array_v1(&self) -> Option<&IndexedArray<Poseidon, usize>> {
        println!(
            "indexed_array_v2: merkle_tree pubkey: {:?}",
            self.accounts.merkle_tree
        );
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V1(_) => Some(&self.indexed_array),
            _ => None,
        }
    }

    pub fn indexed_array_v2(
        &self,
    ) -> Option<&light_indexed_array::array::IndexedArray<Poseidon, usize>> {
        println!(
            "indexed_array_v2: merkle_tree pubkey: {:?}",
            self.accounts.merkle_tree
        );
        match &self.merkle_tree {
            IndexedMerkleTreeVersion::V2(tree) => Some(&tree.indexed_array),
            _ => None,
        }
    }

    pub fn update(
        &mut self,
        new_low_element: &IndexedElement<usize>,
        new_element: &IndexedElement<usize>,
        new_element_next_value: &BigUint,
    ) -> Result<(), IndexerError> {
        match &mut self.merkle_tree {
            IndexedMerkleTreeVersion::V1(tree) => {
                Ok(tree.update(new_low_element, new_element, new_element_next_value)?)
            }
            IndexedMerkleTreeVersion::V2(tree) => Ok(tree.update(
                &light_indexed_array::array::IndexedElement::<usize> {
                    index: new_low_element.index,
                    value: new_low_element.value.clone(),
                    next_index: new_low_element.next_index,
                },
                &light_indexed_array::array::IndexedElement::<usize> {
                    index: new_element.index,
                    value: new_element.value.clone(),
                    next_index: new_element.next_index,
                },
                new_element_next_value,
            )?),
        }
    }
}

pub trait IntoPhotonAccount {
    fn into_photon_account(self) -> Account;
}

pub trait IntoPhotonTokenAccount {
    fn into_photon_token_account(self) -> TokenAccount;
}

impl IntoPhotonAccount for CompressedAccountWithMerkleContext {
    fn into_photon_account(self) -> Account {
        let address = self.compressed_account.address.map(|a| a.to_base58());
        let hash = self.hash().unwrap().to_base58();

        let mut account_data = None;
        if let Some(data) = &self.compressed_account.data {
            let data_bs64 = base64::encode(&*data.data);
            let discriminator = u64::from_be_bytes(data.discriminator);
            account_data = Some(Box::new(photon_api::models::account_data::AccountData {
                data: data_bs64,
                discriminator,
                data_hash: data.data_hash.to_base58(),
            }));
        }

        Account {
            address,
            hash: hash.to_string(),
            lamports: self.compressed_account.lamports,
            data: account_data,
            owner: self.compressed_account.owner.to_string(),
            seq: None,
            slot_created: 0,
            leaf_index: self.merkle_context.leaf_index,
            tree: self.merkle_context.merkle_tree_pubkey.to_string(),
        }
    }
}

impl IntoPhotonTokenAccount for TokenDataWithMerkleContext {
    fn into_photon_token_account(self) -> TokenAccount {
        let base_account = self.compressed_account.into_photon_account();

        let mut tlv = None;
        if let Some(tlv_vec) = &self.token_data.tlv {
            tlv = Some(base64::encode(tlv_vec.as_slice()));
        }

        let token_data = photon_api::models::token_data::TokenData {
            mint: self.token_data.mint.to_string(),
            owner: self.token_data.owner.to_string(),
            amount: self.token_data.amount,
            delegate: self.token_data.delegate.map(|d| d.to_string()),
            state: match self.token_data.state {
                AccountState::Initialized => {
                    photon_api::models::account_state::AccountState::Initialized
                }
                AccountState::Frozen => photon_api::models::account_state::AccountState::Frozen,
            },
            tlv,
        };

        TokenAccount {
            account: Box::new(base_account),
            token_data: Box::new(token_data),
        }
    }
}

pub struct LocalPhotonAccount(Account);

impl TryFrom<LocalPhotonAccount> for CompressedAccountWithMerkleContext {
    type Error = Box<dyn std::error::Error>;

    fn try_from(local_account: LocalPhotonAccount) -> Result<Self, Self::Error> {
        let account = local_account.0;
        let merkle_context = MerkleContext {
            merkle_tree_pubkey: Pubkey::from_str(&account.tree)?,
            queue_pubkey: Default::default(),
            leaf_index: account.leaf_index,
            prove_by_index: false,
            tree_type: light_compressed_account::TreeType::StateV1,
        };

        let mut compressed_account = CompressedAccount {
            address: account
                .address
                .map(|a| <[u8; 32]>::from_base58(&a).unwrap()),
            lamports: account.lamports,
            owner: Pubkey::from_str(&account.owner)?,
            data: None,
        };

        if let Some(data) = account.data {
            let data_decoded = base64::decode(&data.data)?;
            compressed_account.data = Some(CompressedAccountData {
                discriminator: data.discriminator.to_le_bytes(),
                data: data_decoded,
                data_hash: <[u8; 32]>::from_base58(&data.data_hash)?,
            });
        }

        Ok(CompressedAccountWithMerkleContext {
            compressed_account,
            merkle_context,
        })
    }
}

pub trait FromPhotonTokenAccountList {
    fn into_token_data_vec(self) -> Vec<TokenDataWithMerkleContext>;
}

impl FromPhotonTokenAccountList for TokenAccountList {
    fn into_token_data_vec(self) -> Vec<TokenDataWithMerkleContext> {
        self.items
            .into_iter()
            .map(|item| {
                let token_data = TokenData {
                    mint: Pubkey::from_str(&item.token_data.mint).unwrap(),
                    owner: Pubkey::from_str(&item.token_data.owner).unwrap(),
                    amount: item.token_data.amount,
                    delegate: item
                        .token_data
                        .delegate
                        .map(|d| Pubkey::from_str(&d).unwrap()),
                    state: match item.token_data.state {
                        photon_api::models::AccountState::Initialized => AccountState::Initialized,
                        photon_api::models::AccountState::Frozen => AccountState::Frozen,
                    },
                    tlv: item.token_data.tlv.map(|t| base64::decode(&t).unwrap()),
                };

                let compressed_account =
                    CompressedAccountWithMerkleContext::try_from(LocalPhotonAccount(*item.account))
                        .unwrap();

                TokenDataWithMerkleContext {
                    token_data,
                    compressed_account,
                }
            })
            .collect()
    }
}
