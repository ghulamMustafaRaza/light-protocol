use std::fmt::{Debug, Formatter};

use async_trait::async_trait;
use borsh::BorshDeserialize;
#[cfg(feature = "devenv")]
use light_client::fee::{assert_transaction_params, TransactionParams};
use light_client::{
    indexer::Indexer,
    rpc::{merkle_tree::MerkleTreeExt, RpcConnection, RpcError, SolanaRpcConnection},
};
use light_compressed_account::indexer_event::{
    event::{BatchPublicTransactionEvent, PublicTransactionEvent},
    parse::event_from_light_transaction,
};
use light_prover_client::gnark::helpers::{ProverConfig, ProverMode};
use light_sdk::Hash;
use solana_banks_client::BanksClientError;
use solana_program_test::ProgramTestContext;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_sdk::{
    account::{Account, AccountSharedData},
    clock::Slot,
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    system_instruction,
    transaction::{Transaction, TransactionError},
};
use solana_transaction_status::TransactionStatus;

use crate::{
    accounts::env_accounts::EnvAccounts,
    indexer::{TestIndexer, TestIndexerExtensions},
};

pub struct ProgramTestRpcConnection {
    pub context: ProgramTestContext,
    pub indexer: Option<TestIndexer<ProgramTestRpcConnection>>,
    pub env_accounts: EnvAccounts,
}

pub trait TestRpcConnection {
    fn set_account(&mut self, address: &Pubkey, account: &AccountSharedData);
    fn warp_to_slot(
        &mut self,
        slot: Slot,
    ) -> impl std::future::Future<Output = Result<(), RpcError>> + Send;
}

impl TestRpcConnection for SolanaRpcConnection {
    fn set_account(&mut self, _address: &Pubkey, _account: &AccountSharedData) {
        unimplemented!()
    }

    async fn warp_to_slot(&mut self, _slot: Slot) -> Result<(), RpcError> {
        unimplemented!()
    }
}

impl ProgramTestRpcConnection {
    pub fn new(context: ProgramTestContext) -> Self {
        Self {
            context,
            indexer: None,
            env_accounts: EnvAccounts::get_local_test_validator_accounts(),
        }
    }

    pub fn indexer(&self) -> Result<&TestIndexer, RpcError> {
        self.indexer
            .as_ref()
            .ok_or(RpcError::CustomError("Indexer not Initialized".to_string()))
    }

    pub fn indexer_mut(&mut self) -> Result<&mut TestIndexer, RpcError> {
        self.indexer
            .as_mut()
            .ok_or(RpcError::CustomError("Indexer not Initialized".to_string()))
    }

    pub fn env_accounts(&self) -> &EnvAccounts {
        &self.env_accounts
    }

    pub async fn add_indexer(
        &mut self,
        env_accounts: &EnvAccounts,
        with_prover: bool,
    ) -> Result<(), RpcError> {
        let prover_config = if with_prover {
            Some(ProverConfig {
                circuits: vec![],
                run_mode: Some(ProverMode::Rpc),
            })
        } else {
            None
        };
        let indexer =
            TestIndexer::init_from_env(&self.context.payer, env_accounts, prover_config).await;
        self.indexer = Some(indexer);
        Ok(())
    }

    async fn _create_and_send_transaction_with_event<T>(
        &mut self,
        instruction: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
    ) -> Result<Option<(T, Signature, Slot)>, RpcError>
    where
        T: BorshDeserialize + Send + Debug,
    {
        let transaction = Transaction::new_signed_with_payer(
            instruction,
            Some(payer),
            signers,
            self.context.get_new_latest_blockhash().await?,
        );

        let signature = transaction.signatures[0];
        // Simulate the transaction. Currently, in banks-client/server, only
        // simulations are able to track CPIs. Therefore, simulating is the
        // only way to retrieve the event.
        let simulation_result = self
            .context
            .banks_client
            .simulate_transaction(transaction.clone())
            .await?;
        // Handle an error nested in the simulation result.
        if let Some(Err(e)) = simulation_result.result {
            let error = match e {
                TransactionError::InstructionError(_, _) => RpcError::TransactionError(e),
                _ => RpcError::from(BanksClientError::TransactionError(e)),
            };
            return Err(error);
        }
        let event = simulation_result
            .simulation_details
            .and_then(|details| details.inner_instructions)
            .and_then(|instructions| {
                instructions.iter().flatten().find_map(|inner_instruction| {
                    T::try_from_slice(&inner_instruction.instruction.data).ok()
                })
            });
        // If transaction was successful, execute it.
        if let Some(Ok(())) = simulation_result.result {
            let result = self
                .context
                .banks_client
                .process_transaction(transaction)
                .await;
            if let Err(e) = result {
                let error = RpcError::from(e);
                return Err(error);
            }
        }

        let slot = self.context.banks_client.get_root_slot().await?;
        let result = event.map(|event| (event, signature, slot));
        Ok(result)
    }
    async fn _create_and_send_transaction_with_batched_event(
        &mut self,
        instruction: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
    ) -> Result<Option<(Vec<BatchPublicTransactionEvent>, Signature, Slot)>, RpcError> {
        let transaction = Transaction::new_signed_with_payer(
            instruction,
            Some(payer),
            signers,
            self.context.get_new_latest_blockhash().await?,
        );

        self._send_transaction_with_batched_event(transaction).await
    }

    async fn _send_transaction_with_batched_event(
        &mut self,
        transaction: Transaction,
    ) -> Result<Option<(Vec<BatchPublicTransactionEvent>, Signature, Slot)>, RpcError> {
        let mut vec = Vec::new();

        let signature = transaction.signatures[0];
        // Simulate the transaction. Currently, in banks-client/server, only
        // simulations are able to track CPIs. Therefore, simulating is the
        // only way to retrieve the event.
        let simulation_result = self
            .context
            .banks_client
            .simulate_transaction(transaction.clone())
            .await?;
        // Handle an error nested in the simulation result.
        if let Some(Err(e)) = simulation_result.result {
            let error = match e {
                TransactionError::InstructionError(_, _) => RpcError::TransactionError(e),
                _ => RpcError::from(BanksClientError::TransactionError(e)),
            };
            return Err(error);
        }
        let mut vec_accounts = Vec::<Vec<Pubkey>>::new();
        let mut program_ids = Vec::new();

        transaction.message.instructions.iter().for_each(|i| {
            program_ids.push(transaction.message.account_keys[i.program_id_index as usize]);
            vec.push(i.data.clone());
            vec_accounts.push(
                i.accounts
                    .iter()
                    .map(|x| transaction.message.account_keys[*x as usize])
                    .collect(),
            );
        });
        simulation_result
            .simulation_details
            .and_then(|details| details.inner_instructions)
            .and_then(|instructions| {
                instructions.iter().flatten().find_map(|inner_instruction| {
                    vec.push(inner_instruction.instruction.data.clone());
                    program_ids.push(
                        transaction.message.account_keys
                            [inner_instruction.instruction.program_id_index as usize],
                    );
                    vec_accounts.push(
                        inner_instruction
                            .instruction
                            .accounts
                            .iter()
                            .map(|x| transaction.message.account_keys[*x as usize])
                            .collect(),
                    );
                    None::<PublicTransactionEvent>
                })
            });

        let event = event_from_light_transaction(
            program_ids.as_slice(),
            vec.as_slice(),
            vec_accounts.to_vec(),
        )
        .unwrap();
        println!("event: {:?}", event);
        // If transaction was successful, execute it.
        if let Some(Ok(())) = simulation_result.result {
            let result = self
                .context
                .banks_client
                .process_transaction(transaction)
                .await;
            if let Err(e) = result {
                let error = RpcError::from(e);
                return Err(error);
            }
        }

        let slot = self.context.banks_client.get_root_slot().await?;
        let event = event.map(|e| (e, signature, slot));

        if let Some(indexer) = self.indexer.as_mut() {
            if let Some(events) = event.as_ref() {
                for event in events.0.iter() {
                    indexer.add_compressed_accounts_with_token_data(slot, &event.event);
                }
            }
        }

        Ok(event)
    }
}

impl TestRpcConnection for ProgramTestRpcConnection {
    fn set_account(&mut self, address: &Pubkey, account: &AccountSharedData) {
        self.context.set_account(address, account);
    }

    async fn warp_to_slot(&mut self, slot: Slot) -> Result<(), RpcError> {
        self.context
            .warp_to_slot(slot)
            .map_err(|_| RpcError::InvalidWarpSlot)
    }
}

impl Debug for ProgramTestRpcConnection {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProgramTestRpcConnection")
    }
}

#[async_trait]
impl RpcConnection for ProgramTestRpcConnection {
    fn new<U: ToString>(_url: U, _commitment_config: Option<CommitmentConfig>) -> Self
    where
        Self: Sized,
    {
        unimplemented!()
    }

    fn get_payer(&self) -> &Keypair {
        &self.context.payer
    }

    fn get_url(&self) -> String {
        unimplemented!("get_url doesn't make sense for ProgramTestRpcConnection")
    }

    async fn health(&self) -> Result<(), RpcError> {
        Ok(())
    }

    async fn get_program_accounts(
        &self,
        _program_id: &Pubkey,
    ) -> Result<Vec<(Pubkey, Account)>, RpcError> {
        unimplemented!("get_program_accounts")
    }

    async fn process_transaction_with_context(
        &mut self,
        transaction: Transaction,
    ) -> Result<(Signature, Slot), RpcError> {
        let sig = *transaction.signatures.first().unwrap();
        let result = self
            .context
            .banks_client
            .process_transaction_with_metadata(transaction)
            .await
            .map_err(RpcError::from)?;
        result.result.map_err(RpcError::TransactionError)?;
        let slot = self.context.banks_client.get_root_slot().await?;
        Ok((sig, slot))
    }

    async fn confirm_transaction(&self, _transaction: Signature) -> Result<bool, RpcError> {
        Ok(true)
    }

    async fn get_account(&mut self, address: Pubkey) -> Result<Option<Account>, RpcError> {
        self.context
            .banks_client
            .get_account(address)
            .await
            .map_err(RpcError::from)
    }

    async fn get_minimum_balance_for_rent_exemption(
        &mut self,
        data_len: usize,
    ) -> Result<u64, RpcError> {
        let rent = self
            .context
            .banks_client
            .get_rent()
            .await
            .map_err(RpcError::from);

        Ok(rent?.minimum_balance(data_len))
    }

    async fn airdrop_lamports(
        &mut self,
        to: &Pubkey,
        lamports: u64,
    ) -> Result<Signature, RpcError> {
        // Create a transfer instruction
        let transfer_instruction =
            system_instruction::transfer(&self.context.payer.pubkey(), to, lamports);
        let latest_blockhash = self.get_latest_blockhash().await.unwrap();
        // Create and sign a transaction
        let transaction = Transaction::new_signed_with_payer(
            &[transfer_instruction],
            Some(&self.get_payer().pubkey()),
            &vec![&self.get_payer()],
            latest_blockhash,
        );
        let sig = *transaction.signatures.first().unwrap();

        // Send the transaction
        self.context
            .banks_client
            .process_transaction(transaction)
            .await?;

        Ok(sig)
    }

    async fn get_balance(&mut self, pubkey: &Pubkey) -> Result<u64, RpcError> {
        self.context
            .banks_client
            .get_balance(*pubkey)
            .await
            .map_err(RpcError::from)
    }

    async fn get_latest_blockhash(&mut self) -> Result<Hash, RpcError> {
        Ok(self
            .context
            .get_new_latest_blockhash()
            .await
            .map_err(|e| RpcError::from(BanksClientError::from(e)))?
            .to_bytes())
    }

    async fn get_slot(&mut self) -> Result<u64, RpcError> {
        self.context
            .banks_client
            .get_root_slot()
            .await
            .map_err(RpcError::from)
    }

    async fn send_transaction(&self, _transaction: &Transaction) -> Result<Signature, RpcError> {
        Err(RpcError::CustomError(
            "send_transaction is unimplemented for ProgramTestConnection".to_string(),
        ))
    }

    async fn send_transaction_with_config(
        &self,
        _transaction: &Transaction,
        _config: RpcSendTransactionConfig,
    ) -> Result<Signature, RpcError> {
        Err(RpcError::CustomError(
            "send_transaction_with_config is unimplemented for ProgramTestConnection".to_string(),
        ))
    }

    async fn get_transaction_slot(&mut self, signature: &Signature) -> Result<u64, RpcError> {
        self.context
            .banks_client
            .get_transaction_status(*signature)
            .await
            .map_err(RpcError::from)
            .and_then(|status| {
                status
                    .ok_or(RpcError::TransactionError(
                        TransactionError::SignatureFailure,
                    ))
                    .map(|status| status.slot)
            })
    }

    async fn get_signature_statuses(
        &self,
        _signatures: &[Signature],
    ) -> Result<Vec<Option<TransactionStatus>>, RpcError> {
        unimplemented!("get_signature_statuses is unimplemented for ProgramTestRpcConnection")
    }

    #[cfg(not(feature = "devenv"))]
    async fn create_and_send_transaction_with_event<T>(
        &mut self,
        instructions: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
    ) -> Result<Option<(T, Signature, u64)>, RpcError>
    where
        T: BorshDeserialize + Send + Debug,
    {
        self._create_and_send_transaction_with_event::<T>(instructions, payer, signers)
            .await
    }

    #[cfg(not(feature = "devenv"))]
    async fn create_and_send_transaction_with_public_event(
        &mut self,
        instructions: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
    ) -> Result<Option<(PublicTransactionEvent, Signature, Slot)>, RpcError> {
        let parsed_event = self
            ._create_and_send_transaction_with_batched_event(instructions, payer, signers)
            .await?;

        let event = parsed_event.map(|(e, signature, slot)| (e[0].event.clone(), signature, slot));
        Ok(event)
    }

    #[cfg(not(feature = "devenv"))]
    async fn create_and_send_transaction_with_batched_event(
        &mut self,
        instructions: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
    ) -> Result<Option<(Vec<BatchPublicTransactionEvent>, Signature, Slot)>, RpcError> {
        self._create_and_send_transaction_with_batched_event(instructions, payer, signers)
            .await
    }

    #[cfg(feature = "devenv")]
    async fn create_and_send_transaction_with_batched_event(
        &mut self,
        instruction: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
        transaction_params: Option<TransactionParams>,
    ) -> Result<Option<(Vec<BatchPublicTransactionEvent>, Signature, Slot)>, RpcError> {
        let pre_balance = self
            .context
            .banks_client
            .get_account(*payer)
            .await?
            .unwrap()
            .lamports;
        let event = self
            ._create_and_send_transaction_with_batched_event(instruction, payer, signers)
            .await?;

        assert_transaction_params(self, payer, signers, pre_balance, transaction_params).await?;

        Ok(event)
    }

    #[cfg(feature = "devenv")]
    async fn create_and_send_transaction_with_event<T>(
        &mut self,
        instruction: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
        transaction_params: Option<TransactionParams>,
    ) -> Result<Option<(T, Signature, Slot)>, RpcError>
    where
        T: BorshDeserialize + Send + Debug,
    {
        let pre_balance = self
            .context
            .banks_client
            .get_account(*payer)
            .await?
            .unwrap()
            .lamports;

        let result = self
            ._create_and_send_transaction_with_event::<T>(instruction, payer, signers)
            .await?;

        assert_transaction_params(self, payer, signers, pre_balance, transaction_params).await?;

        Ok(result)
    }

    #[cfg(feature = "devenv")]
    async fn create_and_send_transaction_with_public_event(
        &mut self,
        instruction: &[Instruction],
        payer: &Pubkey,
        signers: &[&Keypair],
        transaction_params: Option<TransactionParams>,
    ) -> Result<Option<(PublicTransactionEvent, Signature, Slot)>, RpcError> {
        let res = self
            .create_and_send_transaction_with_batched_event(
                instruction,
                payer,
                signers,
                transaction_params,
            )
            .await?;
        let event = res.map(|e| (e.0[0].event.clone(), e.1, e.2));

        Ok(event)
    }

    async fn process_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<Signature, RpcError> {
        let sig = *transaction.signatures.first().unwrap();
        if self.indexer.is_some() {
            self._send_transaction_with_batched_event(transaction)
                .await?;
        } else {
            self.context
                .banks_client
                .process_transaction(transaction)
                .await
                .map_err(RpcError::from)?;
        }

        Ok(sig)
    }

    async fn indexer(&self) -> Result<&impl Indexer, IndexerError> {
        self.indexer
            .as_ref()
            .ok_or(IndexerError::IndexerNotInitialized)
    }

    async fn get_validity_proof(
        &self,
        hashes: Vec<Hash>,
        new_addresses_with_trees: Vec<AddressWithTree>,
    ) -> Result<ProofRpcResult, IndexerError> {
        let mut state_merkle_tree_pubkeys = Vec::new();

        for hash in hashes.iter() {
            state_merkle_tree_pubkeys.push(Pubkey::from_str_const(
                self.get_compressed_account(*x).await.unwrap().tree.as_str(),
            ));
        }
        let state_merkle_tree_pubkeys = if state_merkle_tree_pubkeys.is_empty() {
            None
        } else {
            Some(state_merkle_tree_pubkeys)
        };
        let hashes = if hashes.is_empty() {
            None
        } else {
            Some(hashes)
        };
        let new_addresses = if new_addresses_with_trees.is_empty() {
            None
        } else {
            Some(new_addresses_with_trees.iter().map(|x| x.address).collect())
        };
        let address_merkle_tree_pubkeys = if new_addresses_with_trees.is_empty() {
            None
        } else {
            Some(new_addresses_with_trees.iter().map(|x| x.tree).collect())
        };
        self.indexer()?
            .create_proof_for_compressed_accounts(
                compressed_accounts,
                state_merkle_tree_pubkeys,
                new_addresses,
                address_merkle_tree_pubkeys,
                rpc,
            )
            .await
    }

    #[cfg(feature = "v2")]
    async fn get_validity_proof_v2(
        &self,
        hashes: Vec<Hash>,
        new_addresses_with_trees: Vec<AddressWithTree>,
    ) -> Result<super::types::ProofRpcResultV2, IndexerError> {
        let mut state_merkle_tree_pubkeys = Vec::new();

        for hash in hashes.iter() {
            state_merkle_tree_pubkeys.push(Pubkey::from_str_const(
                self.get_compressed_account(*x).await.unwrap().tree.as_str(),
            ));
        }
        let state_merkle_tree_pubkeys = if state_merkle_tree_pubkeys.is_empty() {
            None
        } else {
            Some(state_merkle_tree_pubkeys)
        };
        let hashes = if hashes.is_empty() {
            None
        } else {
            Some(hashes)
        };
        let new_addresses = if new_addresses_with_trees.is_empty() {
            None
        } else {
            Some(new_addresses_with_trees.iter().map(|x| x.address).collect())
        };
        let address_merkle_tree_pubkeys = if new_addresses_with_trees.is_empty() {
            None
        } else {
            Some(new_addresses_with_trees.iter().map(|x| x.tree).collect())
        };
        self.indexer()?
            .create_proof_for_compressed_accounts2(
                compressed_accounts,
                state_merkle_tree_pubkeys,
                new_addresses,
                address_merkle_tree_pubkeys,
                rpc,
            )
            .await
    }
}

impl MerkleTreeExt for ProgramTestRpcConnection {}
