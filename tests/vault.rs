//! LiteSVM integration tests. Build the program first: `cargo build-sbf`.

use litesvm::LiteSVM;
use pinocchio_vault::ID as PROGRAM_ID;
use solana_keypair::Keypair;
use solana_signer::Signer;
use solana_system_interface::instruction as system_ix;
use solana_transaction::{
    AccountMeta, Address, Instruction, InstructionError, Transaction, TransactionError,
};

const SYSTEM_PROGRAM_ID: Address = Address::new_from_array([0; 32]);
const VAULT_SIZE: usize = 16;
const SOL: u64 = 1_000_000_000;

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(
        PROGRAM_ID,
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/target/deploy/pinocchio_vault.so"
        ),
    )
    .expect("run `cargo build-sbf` before `cargo test`");
    let owner = Keypair::new();
    svm.airdrop(&owner.pubkey(), 10 * SOL).expect("airdrop");
    (svm, owner)
}

fn vault_of(owner: &Address) -> Address {
    Address::find_program_address(&[b"vault", owner.as_ref()], &PROGRAM_ID).0
}

fn deposit_ix(owner: &Address, vault: &Address, amount: u64) -> Instruction {
    let mut data = vec![0u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &data,
        vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        ],
    )
}

fn withdraw_ix(owner: &Address, vault: &Address) -> Instruction {
    Instruction::new_with_bytes(
        PROGRAM_ID,
        &[1u8],
        vec![
            AccountMeta::new(*owner, true),
            AccountMeta::new(*vault, false),
        ],
    )
}

fn send(svm: &mut LiteSVM, ix: Instruction, payer: &Keypair) -> Result<(), TransactionError> {
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx).map(|_| ()).map_err(|e| e.err);
    svm.expire_blockhash();
    result
}

fn ix_err(err: InstructionError) -> Result<(), TransactionError> {
    Err(TransactionError::InstructionError(0, err))
}

#[test]
fn deposit_then_withdraw_round_trip() {
    let (mut svm, owner) = setup();
    let vault = vault_of(&owner.pubkey());
    let rent_min = svm.minimum_balance_for_rent_exemption(VAULT_SIZE);

    send(&mut svm, deposit_ix(&owner.pubkey(), &vault, SOL), &owner).unwrap();
    let account = svm.get_account(&vault).unwrap();
    assert_eq!(account.owner, PROGRAM_ID);
    assert_eq!(account.data.len(), VAULT_SIZE);
    assert_eq!(account.lamports, rent_min + SOL);

    // Second deposit reuses the existing vault.
    send(&mut svm, deposit_ix(&owner.pubkey(), &vault, SOL), &owner).unwrap();
    assert_eq!(svm.get_balance(&vault).unwrap(), rent_min + 2 * SOL);

    let before = svm.get_balance(&owner.pubkey()).unwrap();
    send(&mut svm, withdraw_ix(&owner.pubkey(), &vault), &owner).unwrap();
    assert_eq!(svm.get_balance(&vault).unwrap(), rent_min);
    // Owner gets 2 SOL back minus the 5000-lamport tx fee.
    assert_eq!(
        svm.get_balance(&owner.pubkey()).unwrap(),
        before + 2 * SOL - 5000
    );

    // Nothing left above the rent minimum.
    assert_eq!(
        send(&mut svm, withdraw_ix(&owner.pubkey(), &vault), &owner),
        ix_err(InstructionError::InsufficientFunds)
    );
}

#[test]
fn prefunded_vault_cannot_block_creation() {
    // Smallest viable grief (rent minimum for a 0-byte system account, still below the
    // vault's own rent minimum) and a prefund larger than the vault needs.
    let min_system_account = LiteSVM::new().minimum_balance_for_rent_exemption(0);
    for prefund in [min_system_account, 10 * SOL] {
        let (mut svm, owner) = setup();
        let attacker = Keypair::new();
        svm.airdrop(&attacker.pubkey(), 20 * SOL).unwrap();
        let vault = vault_of(&owner.pubkey());
        let rent_min = svm.minimum_balance_for_rent_exemption(VAULT_SIZE);

        // Attacker sends lamports to the PDA before the owner's first deposit.
        let grief = system_ix::transfer(&attacker.pubkey(), &vault, prefund);
        send(&mut svm, grief, &attacker).unwrap();

        send(&mut svm, deposit_ix(&owner.pubkey(), &vault, SOL), &owner).unwrap();
        let account = svm.get_account(&vault).unwrap();
        assert_eq!(account.owner, PROGRAM_ID);
        assert_eq!(account.data.len(), VAULT_SIZE);
        assert_eq!(account.lamports, rent_min.max(prefund) + SOL);
    }
}

#[test]
fn withdraw_rejects_prefunded_uninitialized_vault() {
    let (mut svm, owner) = setup();
    let vault = vault_of(&owner.pubkey());
    send(
        &mut svm,
        system_ix::transfer(&owner.pubkey(), &vault, SOL),
        &owner,
    )
    .unwrap();

    assert_eq!(
        send(&mut svm, withdraw_ix(&owner.pubkey(), &vault), &owner),
        ix_err(InstructionError::InvalidAccountOwner)
    );
}

#[test]
fn deposit_rejects_someone_elses_vault() {
    let (mut svm, victim) = setup();
    let victim_vault = vault_of(&victim.pubkey());
    send(
        &mut svm,
        deposit_ix(&victim.pubkey(), &victim_vault, SOL),
        &victim,
    )
    .unwrap();

    let other = Keypair::new();
    svm.airdrop(&other.pubkey(), 10 * SOL).unwrap();
    assert_eq!(
        send(
            &mut svm,
            deposit_ix(&other.pubkey(), &victim_vault, SOL),
            &other
        ),
        ix_err(InstructionError::InvalidSeeds)
    );
}

#[test]
fn withdraw_rejects_someone_elses_vault() {
    let (mut svm, victim) = setup();
    let victim_vault = vault_of(&victim.pubkey());
    send(
        &mut svm,
        deposit_ix(&victim.pubkey(), &victim_vault, SOL),
        &victim,
    )
    .unwrap();

    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), SOL).unwrap();
    assert_eq!(
        send(
            &mut svm,
            withdraw_ix(&attacker.pubkey(), &victim_vault),
            &attacker
        ),
        ix_err(InstructionError::InvalidSeeds)
    );
}

#[test]
fn withdraw_requires_owner_signature() {
    let (mut svm, owner) = setup();
    let vault = vault_of(&owner.pubkey());
    send(&mut svm, deposit_ix(&owner.pubkey(), &vault, SOL), &owner).unwrap();

    // Someone else pays and signs, owner is passed as a non-signer.
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), SOL).unwrap();
    let mut ix = withdraw_ix(&owner.pubkey(), &vault);
    ix.accounts[0].is_signer = false;
    assert_eq!(
        send(&mut svm, ix, &payer),
        ix_err(InstructionError::MissingRequiredSignature)
    );
}

#[test]
fn deposit_rejects_fake_system_program() {
    let (mut svm, owner) = setup();
    let vault = vault_of(&owner.pubkey());
    let mut ix = deposit_ix(&owner.pubkey(), &vault, SOL);
    ix.accounts[2].pubkey = PROGRAM_ID;
    assert_eq!(
        send(&mut svm, ix, &owner),
        ix_err(InstructionError::IncorrectProgramId)
    );
}

#[test]
fn deposit_rejects_bad_amounts() {
    let (mut svm, owner) = setup();
    let vault = vault_of(&owner.pubkey());
    assert_eq!(
        send(&mut svm, deposit_ix(&owner.pubkey(), &vault, 0), &owner),
        ix_err(InstructionError::InvalidInstructionData)
    );

    let mut ix = deposit_ix(&owner.pubkey(), &vault, SOL);
    ix.data.pop();
    assert_eq!(
        send(&mut svm, ix, &owner),
        ix_err(InstructionError::InvalidInstructionData)
    );
}
