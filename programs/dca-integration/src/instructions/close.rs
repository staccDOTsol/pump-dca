use crate::constants::{AIRDROP_BPS, ESCROW_SEED};
use crate::{errors::EscrowErrors, escrow_seeds, math, state::Escrow};
use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount, Transfer},
};

#[derive(Accounts)]
pub struct Close<'info> {
    #[account(
      address=escrow.input_mint
    )]
    input_mint: Box<Account<'info, Mint>>,

    #[account(
      address=escrow.output_mint
    )]
    output_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    user: Signer<'info>,

    #[account(
      mut,
      constraint=escrow.user==user.key(),
    )]
    escrow: Box<Account<'info, Escrow>>,

    #[account(
      mut,
      associated_token::authority=escrow,
      associated_token::mint=input_mint,
    )]
    escrow_in_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Not mutating and checks that it belongs to this user
    #[account(
      address=escrow.dca
    )]
    dca: UncheckedAccount<'info>,

    #[account(
      mut,
      associated_token::authority=escrow,
      associated_token::mint=output_mint,
    )]
    escrow_out_ata: Box<Account<'info, TokenAccount>>,

    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
    associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> Close<'info> {
    pub fn compute_airdrop_amount(out_amount: u64) -> Result<u64> {
        let u128_amount = math::checked_div(
            math::checked_mul(out_amount as u128, AIRDROP_BPS as u128)?,
            10000,
        )?;
        let u64_amount: u64 = math::checked_as_u64(u128_amount)?;

        Ok(u64_amount)
    }
}

pub fn close(ctx: Context<Close>) -> Result<()> {
    // Checks that the DCA account is done and closed before closing escrow account
    require_eq!(ctx.accounts.dca.lamports(), 0, EscrowErrors::DCANotClosed);

    require_eq!(
        ctx.accounts.escrow_in_ata.amount,
        0,
        EscrowErrors::UnexpectedBalance
    );

    let escrow = &mut ctx.accounts.escrow;
    escrow.output_amount = ctx.accounts.escrow_out_ata.amount; // will this work for native SOL?
    escrow.completed = true;
    escrow.airdrop_amount = Close::compute_airdrop_amount(ctx.accounts.escrow_out_ata.amount)?;

    let idx_bytes = ctx.accounts.escrow.idx.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[escrow_seeds!(ctx.accounts.escrow, idx_bytes)];

    // Burn tokens instead of closing accounts
    anchor_spl::token::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::Burn {
                mint: ctx.accounts.input_mint.to_account_info(),
                from: ctx.accounts.escrow_in_ata.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            signer_seeds,
        ),
        ctx.accounts.escrow_in_ata.amount,
    )?;

    anchor_spl::token::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            anchor_spl::token::Burn {
                mint: ctx.accounts.output_mint.to_account_info(),
                from: ctx.accounts.escrow_out_ata.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            signer_seeds,
        ),
        ctx.accounts.escrow_out_ata.amount,
    )?;

    Ok(())
}
