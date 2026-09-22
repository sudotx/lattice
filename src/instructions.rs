// instructions.rs
use core::convert::TryFrom;
use core::mem::size_of;
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_log::log;
use pinocchio_system::{
    create_account_with_minimum_balance_signed, instructions::Transfer as SystemTransfer,
};
use shank::ShankInstruction;

// instructions.rs
/// Shank IDL facade enum describing all program instructions and their required accounts.
/// This is used only for IDL generation and does not affect runtime behavior.
#[derive(ShankInstruction)]
pub enum ProgramIx {
    /// Deposit lamports into the vault.
    #[account(0, signer, writable, name = "owner", desc = "Vault owner and payer")]
    #[account(1, writable, name = "vault", desc = "Vault PDA for lamports")]
    #[account(2, name = "system_program", desc = "System Program Address")]
    Deposit { amount: u64 },

    /// Withdraw all lamports above the rent-exempt minimum back to the owner.
    #[account(
        0,
        signer,
        writable,
        name = "owner",
        desc = "Vault owner and authority"
    )]
    #[account(1, writable, name = "vault", desc = "Vault PDA for lamports")]
    Withdraw {},
}

const VAULT_SEED: &[u8] = b"vault";

/// Vault account size. The data is currently unused; the vault only holds lamports.
const VAULT_SIZE: usize = 8 + size_of::<u64>();

// instructions.rs
/// Parse a non-zero u64 from instruction data.
fn parse_amount(data: &[u8]) -> Result<u64, ProgramError> {
    let bytes: [u8; 8] = data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let amt = u64::from_le_bytes(bytes);
    if amt == 0 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(amt)
}

/// Check that `vault` is the canonical vault PDA for `owner` and return its bump.
fn verify_vault_address(owner: &AccountView, vault: &AccountView) -> Result<u8, ProgramError> {
    let (expected, bump) =
        Address::find_program_address(&[VAULT_SEED, owner.address().as_ref()], &crate::ID);
    if vault.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    Ok(bump)
}

/// Ensure the vault exists; if not, create it with PDA seeds.
///
/// A vault that is not yet owned by this program is initialized even when it already
/// holds lamports, so pre-funding the PDA address cannot block its creation.
fn ensure_vault_exists(owner: &AccountView, vault: &mut AccountView, bump: u8) -> ProgramResult {
    if vault.owned_by(&crate::ID) {
        log!("Vault already exists");
        return Ok(());
    }
    let bump = [bump];
    let signer_seeds = [
        Seed::from(VAULT_SEED),
        Seed::from(owner.address().as_ref()),
        Seed::from(&bump),
    ];
    let signer = Signer::from(&signer_seeds);

    // Uses CreateAccount when the vault is empty, otherwise tops up the rent deficit
    // and runs Allocate + Assign. Both paths fail unless the vault is system-owned.
    create_account_with_minimum_balance_signed(
        vault,
        VAULT_SIZE,
        &crate::ID,
        owner,
        None,
        &[signer],
    )?;

    log!("Vault created");
    Ok(())
}

// instructions.rs
pub struct Deposit<'a> {
    pub owner: &'a AccountView,
    pub vault: &'a mut AccountView,
    pub amount: u64,
    pub bump: u8,
}

impl Deposit<'_> {
    pub const DISCRIMINATOR: u8 = 0;

    pub fn process(self) -> ProgramResult {
        let Deposit {
            owner,
            vault,
            amount,
            bump,
        } = self;

        ensure_vault_exists(owner, vault, bump)?;

        SystemTransfer {
            from: owner,
            to: vault,
            lamports: amount,
        }
        .invoke()?;
        log!("{} Lamports deposited to vault", amount);
        Ok(())
    }
}

impl<'a> TryFrom<(&'a [u8], &'a mut [AccountView])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from(value: (&'a [u8], &'a mut [AccountView])) -> Result<Self, Self::Error> {
        let (data, accounts) = value;
        let [owner, vault, system_program, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        if !owner.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let bump = verify_vault_address(owner, vault)?;

        if system_program.address() != &pinocchio_system::ID {
            return Err(ProgramError::IncorrectProgramId);
        }

        let amount = parse_amount(data)?;
        Ok(Self {
            owner,
            vault,
            amount,
            bump,
        })
    }
}

// instructions.rs
pub struct Withdraw<'a> {
    pub owner: &'a mut AccountView,
    pub vault: &'a mut AccountView,
}

impl Withdraw<'_> {
    pub const DISCRIMINATOR: u8 = 1;

    /// Transfer lamports from the vault PDA to the owner, leaving the rent minimum in place.
    pub fn process(self) -> ProgramResult {
        let Withdraw { owner, vault } = self;

        // Compute how much can be withdrawn while keeping the account rent-exempt
        let min_balance = Rent::get()?.try_minimum_balance(vault.data_len())?;
        let current = vault.lamports();
        let Some(withdraw_amount) = current.checked_sub(min_balance).filter(|amt| *amt > 0) else {
            // Nothing withdrawable; keep behavior strict to avoid rent violations
            return Err(ProgramError::InsufficientFunds);
        };

        let owner_lamports = owner
            .lamports()
            .checked_add(withdraw_amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // The vault is program-owned, so its lamports can be debited directly.
        vault.set_lamports(min_balance);
        owner.set_lamports(owner_lamports);

        log!("{} lamports withdrawn from vault", withdraw_amount);
        Ok(())
    }
}

impl<'a> TryFrom<&'a mut [AccountView]> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a mut [AccountView]) -> Result<Self, Self::Error> {
        let [owner, vault, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        if !owner.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // Validate that the vault is owned by the program
        if !vault.owned_by(&crate::ID) {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Validate that the provided vault account is the correct PDA for this owner
        verify_vault_address(owner, vault)?;

        Ok(Self { owner, vault })
    }
}
