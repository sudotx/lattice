// lib.rs
#![no_std]

use pinocchio::{entrypoint, error::ProgramError, AccountView, Address, ProgramResult};
use solana_address::declare_id;

entrypoint!(process_instruction);

pub mod instructions;
pub use instructions::*;

declare_id!("G9S9ELuKARYok1H8QguVSZ5mu7VoT3FfRBqeFL3vK3uW");

fn process_instruction(
    _program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((&Deposit::DISCRIMINATOR, data)) => Deposit::try_from((data, accounts))?.process(),
        Some((&Withdraw::DISCRIMINATOR, _)) => Withdraw::try_from(accounts)?.process(),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
