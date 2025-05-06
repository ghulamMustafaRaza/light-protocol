use account_compression::{
    AddressMerkleTreeConfig, AddressQueueConfig, NullifierQueueConfig, StateMerkleTreeConfig,
};
use forester_utils::utils::airdrop_lamports;
use light_batched_merkle_tree::{
    initialize_address_tree::InitAddressTreeAccountsInstructionData,
    initialize_state_tree::InitStateTreeAccountsInstructionData,
};
use light_registry::{
    account_compression_cpi::sdk::get_registered_program_pda,
    protocol_config::state::ProtocolConfig,
};
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::{pubkey::Pubkey, signature::Signer};

use crate::{
    accounts::{
        env_accounts::{EnvAccounts, NOOP_PROGRAM_ID},
        env_keypairs::EnvAccountKeypairs,
        initialize::initialize_accounts,
        registered_program_accounts,
    },
    find_light_bin::find_light_bin,
    test_rpc::ProgramTestRpcConnection,
};

pub const CPI_CONTEXT_ACCOUNT_RENT: u64 = 143487360; // lamports of the cpi context account

pub struct ProgramTestConfig {
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
    protocol_config: ProtocolConfig,
    register_forester_and_advance_to_active_phase: bool,
    batched_tree_init_params: InitStateTreeAccountsInstructionData,
    batched_address_tree_init_params: InitAddressTreeAccountsInstructionData,
    with_prover: bool,
}

impl ProgramTestConfig {
    pub fn new(
        additional_programs: Option<Vec<(&'static str, Pubkey)>>,
        with_prover: bool,
    ) -> Self {
        Self {
            additional_programs,
            protocol_config: ProtocolConfig::default(),
            register_forester_and_advance_to_active_phase: true,
            batched_tree_init_params: InitStateTreeAccountsInstructionData::test_default(),
            batched_address_tree_init_params: InitAddressTreeAccountsInstructionData::test_default(
            ),
            with_prover,
        }
    }
}

impl Default for ProgramTestConfig {
    fn default() -> Self {
        Self {
            additional_programs: None,
            protocol_config: ProtocolConfig::default(),
            register_forester_and_advance_to_active_phase: true,
            batched_tree_init_params: InitStateTreeAccountsInstructionData::default(),
            batched_address_tree_init_params: InitAddressTreeAccountsInstructionData::default(),
            with_prover: true,
        }
    }
}

/// Setup test programs
/// deploys:
/// 1. light_registry program
/// 2. account_compression program
/// 3. light_compressed_token program
/// 4. light_system_program program
pub async fn setup_test_programs(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
) -> ProgramTestContext {
    let mut program_test = ProgramTest::default();
    let sbf_path = std::env::var("SBF_OUT_DIR").unwrap();
    // find path to bin where light cli stores program binaries.
    let path = find_light_bin().unwrap();
    std::env::set_var("SBF_OUT_DIR", path.to_str().unwrap());
    program_test.add_program("light_registry", light_registry::ID, None);
    program_test.add_program("account_compression", account_compression::ID, None);
    program_test.add_program("light_compressed_token", light_compressed_token::ID, None);
    program_test.add_program(
        "light_system_program_pinocchio",
        light_system_program::ID,
        None,
    );
    program_test.add_program("spl_noop", NOOP_PROGRAM_ID, None);
    std::env::set_var("SBF_OUT_DIR", sbf_path);
    let registered_program = registered_program_accounts::get_registered_program_pda();
    program_test.add_account(
        get_registered_program_pda(&light_system_program::ID),
        registered_program,
    );
    let registered_program = registered_program_accounts::get_registered_registry_program_pda();
    program_test.add_account(
        get_registered_program_pda(&light_registry::ID),
        registered_program,
    );
    if let Some(programs) = additional_programs {
        for (name, id) in programs {
            program_test.add_program(name, id, None);
        }
    }
    program_test.set_compute_max_units(1_400_000u64);
    program_test.start_with_context().await
}

/// Setup test programs with accounts
/// deploys:
/// 1. light program
/// 2. account_compression program
/// 3. light_compressed_token program
/// 4. light_system_program program
///
/// Sets up the following accounts:
/// 5. creates and initializes governance authority
/// 6. creates and initializes group authority
/// 7. registers the light_system_program program with the group authority
/// 8. initializes Merkle tree owned by
/// Note:
/// - registers a forester
/// - advances to the active phase slot 2
/// - active phase doesn't end
// TODO(vadorovsky): Remove this function...
pub async fn setup_test_programs_with_accounts(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    setup_test_programs_with_accounts_with_protocol_config(
        additional_programs,
        ProtocolConfig {
            // Init with an active epoch which doesn't end
            active_phase_length: 1_000_000_000,
            slot_length: 1_000_000_000 - 1,
            genesis_slot: 0,
            registration_phase_length: 2,
            ..Default::default()
        },
        true,
    )
    .await
}

/// Setup test programs with accounts
/// deploys:
/// 1. light program
/// 2. account_compression program
/// 3. light_compressed_token program
/// 4. light_system_program program
///
/// Sets up the following accounts:
/// 5. creates and initializes governance authority
/// 6. creates and initializes group authority
/// 7. registers the light_system_program program with the group authority
/// 8. initializes Merkle tree owned by
/// Note:
/// - registers a forester
/// - advances to the active phase slot 2
/// - active phase doesn't end
pub async fn setup_test_programs_with_accounts_v2(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    setup_test_programs_with_accounts_with_protocol_config_v2(
        additional_programs,
        ProtocolConfig {
            // Init with an active epoch which doesn't end
            active_phase_length: 1_000_000_000,
            slot_length: 1_000_000_000 - 1,
            genesis_slot: 0,
            registration_phase_length: 2,
            ..Default::default()
        },
        true,
    )
    .await
}

pub async fn setup_test_programs_with_accounts_with_protocol_config(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
    protocol_config: ProtocolConfig,
    register_forester_and_advance_to_active_phase: bool,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    setup_test_programs_with_accounts_with_protocol_config_and_batched_tree_params(
        additional_programs,
        protocol_config,
        register_forester_and_advance_to_active_phase,
        InitStateTreeAccountsInstructionData::test_default(),
        InitAddressTreeAccountsInstructionData::test_default(),
    )
    .await
}

pub async fn setup_test_programs_with_accounts_with_protocol_config_and_batched_tree_params(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
    protocol_config: ProtocolConfig,
    register_forester_and_advance_to_active_phase: bool,
    batched_tree_init_params: InitStateTreeAccountsInstructionData,
    batched_address_tree_init_params: InitAddressTreeAccountsInstructionData,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    common_setup_test_programs(ProgramTestConfig {
        additional_programs,
        protocol_config,
        register_forester_and_advance_to_active_phase,
        batched_tree_init_params,
        batched_address_tree_init_params,
        with_prover: false,
    })
    .await
}

pub async fn common_setup_test_programs(
    config: ProgramTestConfig,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    let ProgramTestConfig {
        additional_programs,
        protocol_config,
        register_forester_and_advance_to_active_phase,
        batched_tree_init_params,
        batched_address_tree_init_params,
        with_prover,
    } = config;
    let context = setup_test_programs(additional_programs).await;
    let mut context = ProgramTestRpcConnection::new(context);
    let keypairs = EnvAccountKeypairs::program_test_default();
    println!(
        "batched cpi context pubkey : {:?}",
        keypairs.batched_cpi_context.pubkey()
    );
    println!(
        "batched cpi context pubkey : {:?}",
        keypairs.batched_cpi_context.pubkey().to_bytes()
    );

    airdrop_lamports(
        &mut context,
        &keypairs.governance_authority.pubkey(),
        100_000_000_000,
    )
    .await
    .unwrap();
    airdrop_lamports(&mut context, &keypairs.forester.pubkey(), 10_000_000_000)
        .await
        .unwrap();
    let env_accounts = initialize_accounts(
        &mut context,
        keypairs,
        protocol_config,
        register_forester_and_advance_to_active_phase,
        true,
        false,
        StateMerkleTreeConfig::default(),
        NullifierQueueConfig::default(),
        AddressMerkleTreeConfig::default(),
        AddressQueueConfig::default(),
        batched_tree_init_params,
        Some(batched_address_tree_init_params),
    )
    .await;
    context
        .add_indexer(&env_accounts, with_prover)
        .await
        .unwrap();
    (context, env_accounts)
}

// TODO(vadorovsky): ...in favor of this one.
pub async fn setup_test_programs_with_accounts_with_protocol_config_v2(
    additional_programs: Option<Vec<(&'static str, Pubkey)>>,
    protocol_config: ProtocolConfig,
    register_forester_and_advance_to_active_phase: bool,
) -> (ProgramTestRpcConnection, EnvAccounts) {
    let context = setup_test_programs(additional_programs).await;
    let mut context = ProgramTestRpcConnection::new(context);
    let keypairs = EnvAccountKeypairs::program_test_default();
    airdrop_lamports(
        &mut context,
        &keypairs.governance_authority.pubkey(),
        100_000_000_000,
    )
    .await
    .unwrap();
    airdrop_lamports(&mut context, &keypairs.forester.pubkey(), 10_000_000_000)
        .await
        .unwrap();
    let params = InitStateTreeAccountsInstructionData::test_default();
    let env_accounts = initialize_accounts(
        &mut context,
        keypairs,
        protocol_config,
        register_forester_and_advance_to_active_phase,
        true,
        false,
        StateMerkleTreeConfig::default(),
        NullifierQueueConfig::default(),
        AddressMerkleTreeConfig::default(),
        AddressQueueConfig::default(),
        params,
        Some(InitAddressTreeAccountsInstructionData::test_default()),
    )
    .await;
    (context, env_accounts)
}
