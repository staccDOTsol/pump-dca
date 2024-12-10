use std::str::FromStr;

use crate::constants::ESCROW_SEED;
use crate::{escrow_seeds, state::Escrow};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_lang::system_program::{transfer, Transfer as SystemTransfer};
use anchor_spl::token::spl_token::native_mint;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount, Transfer},
};
use jupiter_dca::cpi::{self};
use solana_program::program::invoke;

#[derive(Accounts)]
#[instruction(application_idx: u64)]
pub struct SetupDca<'info> {
    /// CHECK: Jup DCA will check
    jup_dca_program: UncheckedAccount<'info>,

    /// CHECK: Jup DCA will check
    #[account(mut)]
    jup_dca: UncheckedAccount<'info>,

    /// CHECK: Jup DCA will check
    #[account(mut)]
    jup_dca_in_ata: UncheckedAccount<'info>,

    /// CHECK: Jup DCA will check
    #[account(mut)]
    jup_dca_out_ata: UncheckedAccount<'info>,

    /// CHECK: Jup DCA will check
    jup_dca_event_authority: UncheckedAccount<'info>,
    #[account(address = native_mint::ID)]
    input_mint: Box<Account<'info, Mint>>,
 

    output_mint: Box<Account<'info, Mint>>,

    #[account(mut)]
    user: Signer<'info>,

    #[account(
      init,
      payer = user,
      space = Escrow::LEN,
      seeds = [ESCROW_SEED, user.key().as_ref(), input_mint.key().as_ref(), output_mint.key().as_ref(), application_idx.to_le_bytes().as_ref()],
      bump
    )]
    escrow: Box<Account<'info, Escrow>>,

    #[account(mut,
        seeds = [b"heehee", output_mint.key().as_ref()],
        bump
    )]
    heehee: SystemAccount<'info>,

    #[account(mut
    )]
    escrow_in_ata: Signer<'info>,

    #[account(mut
    )]
    escrow_out_ata: Box<Account<'info, TokenAccount>>,

    system_program: Program<'info, System>,
    token_program: Program<'info, Token>,
    associated_token_program: Program<'info, AssociatedToken>,
    rent: Sysvar<'info, Rent>,
}

pub fn setup_dca(
    ctx: Context<SetupDca>,
    application_idx: u64,
    _in_amount: u64,
    _in_amount_per_cycle: u64,
    _cycle_frequency: i64,
    min_out_amount: Option<u64>,
    max_out_amount: Option<u64>,
    start_at: Option<i64>,
) -> Result<()> {
    msg!("Unwrap native SOL from escrow in ata to wrapped SOL");
    let binding = application_idx.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[&[
        ESCROW_SEED,
        ctx.accounts.user.to_account_info().key.as_ref(),
        ctx.accounts.input_mint.to_account_info().key.as_ref(),
        ctx.accounts.output_mint.to_account_info().key.as_ref(),
        binding.as_ref(),
        &[ctx.bumps["escrow"]]
    ]];
    msg!("bruh transfer the lamports from heehee to escrow in ata");
    
    msg!("Getting rent-exempt minimum for heehee account");
    let rent = Rent::get()?;
    let min_rent = rent.minimum_balance(0);
    
    msg!("Calculating transferable lamports, leaving {} for rent-exemption", min_rent);
    let lamport_ss = ctx.accounts.heehee.lamports()
        .checked_sub(min_rent)
        .ok_or(ErrorCode::InvalidProgramExecutable)?;  // Changed error type to be more specific
    
    // Add minimum amount check
    require!(lamport_ss >= 100, ErrorCode::InvalidProgramExecutable);  // Ensure we have enough for at least 100 cycles
    
    msg!("Initializing escrow_in_ata");

    let heehee   = &mut ctx.accounts.heehee.to_account_info();
    let escrow_in_ata = &mut &mut ctx.accounts.escrow_in_ata.to_account_info();
    let user = &mut ctx.accounts.user.to_account_info();

    let heehee_signer_seeds: &[&[&[u8]]] = &[&[
        b"heehee",
        ctx.accounts.output_mint.to_account_info().key.as_ref(),
        &[ctx.bumps["heehee"]]
    ]];
    msg!("Creating escrow_in_ata account");
    let create_account_ix = solana_program::system_instruction::create_account(
        &ctx.accounts.heehee.key(),
        &escrow_in_ata.key(),
        min_rent,
        165, // Token account size
        &anchor_spl::token::ID
    );
    invoke_signed(
        &create_account_ix,
        &[
            heehee.clone(),
            escrow_in_ata.clone(),
        ],
        heehee_signer_seeds
    )?;
    
    msg!("transferring lamports from user to escrow_in_ata");

    transfer(CpiContext::new_with_signer(ctx.accounts.system_program.to_account_info(),
    SystemTransfer {
        from: heehee.clone(),
        to: escrow_in_ata.clone(),
    },heehee_signer_seeds), lamport_ss)?;
    msg!("Initializing token account");
    let init_ix = spl_token::instruction::initialize_account(
        &anchor_spl::token::ID,
        escrow_in_ata.key,
        ctx.accounts.input_mint.to_account_info().key,
        ctx.accounts.escrow.to_account_info().key,
    )?;

    invoke_signed(
        &init_ix,
        &[
            escrow_in_ata.clone(),
            ctx.accounts.input_mint.to_account_info().clone(),
            ctx.accounts.escrow.to_account_info().clone(),
            ctx.accounts.system_program.to_account_info().clone(),
            ctx.accounts.rent.to_account_info().clone(),
        ],
        signer_seeds,
    )?;

    msg!("Syncing native account");
    let sync_native_ix = spl_token::instruction::sync_native(
        &anchor_spl::token::ID,
        escrow_in_ata.key,
    )?;

    invoke_signed(
        &sync_native_ix,
        &[escrow_in_ata.clone(), ctx.accounts.token_program.to_account_info().clone(), ctx.accounts.system_program.to_account_info().clone()],
        signer_seeds,
    )?;

    msg!("Calculating DCA parameters");
    let in_amount_per_cycle = lamport_ss
        .checked_div(100)
        .ok_or(ErrorCode::InvalidProgramExecutable)?;  // Safe division
    let cycle_frequency = 60 ; // hour in seconds

    msg!("Will DCA {} lamports per cycle every {} seconds", in_amount_per_cycle, cycle_frequency);

    msg!("Initializing escrow account");
    let escrow = &mut ctx.accounts.escrow;
    escrow.idx = application_idx;
    escrow.user = *ctx.accounts.user.key;
    escrow.dca = ctx.accounts.jup_dca.key();
    escrow.input_mint = ctx.accounts.input_mint.key();
    escrow.output_mint = ctx.accounts.output_mint.key();
    escrow.input_amount = lamport_ss;
    escrow.output_amount = 0;
    escrow.airdrop_amount = 0;
    escrow.completed = false;
    escrow.airdropped = false;
    escrow.bump = *ctx.bumps.get("escrow").unwrap();
    msg!("Escrow initialized with idx {} for user {}", application_idx, escrow.user);

    msg!("Constructing open DCA context");
    let idx_bytes = ctx.accounts.escrow.idx.to_le_bytes();
    let signer_seeds: &[&[&[u8]]] = &[escrow_seeds!(ctx.accounts.escrow, idx_bytes)];
    let open_dca_accounts = cpi::accounts::OpenDcaV2 {
        input_mint: ctx.accounts.input_mint.to_account_info(),
        output_mint: ctx.accounts.output_mint.to_account_info(),
        dca: ctx.accounts.jup_dca.to_account_info(),
        payer: ctx.accounts.user.to_account_info(),
        user: ctx.accounts.escrow.to_account_info(),
        user_ata: ctx.accounts.escrow_in_ata.to_account_info(),
        in_ata: ctx.accounts.jup_dca_in_ata.to_account_info(),
        out_ata: ctx.accounts.jup_dca_out_ata.to_account_info(),
        event_authority: ctx.accounts.jup_dca_event_authority.to_account_info(),
        program: ctx.accounts.jup_dca_program.to_account_info(),
        system_program: ctx.accounts.system_program.to_account_info(),
        token_program: ctx.accounts.token_program.to_account_info(),
        associated_token_program: ctx.accounts.associated_token_program.to_account_info(),
    };
    let cpi_ctx = CpiContext::new_with_signer(
        ctx.accounts.jup_dca.to_account_info(),
        open_dca_accounts,
        signer_seeds,
    );

    msg!("Making CPI call to open DCA with {} total lamports, {} per cycle", lamport_ss, in_amount_per_cycle);
    cpi::open_dca_v2(
        cpi_ctx,
        application_idx,
        lamport_ss- min_rent- min_rent,
        in_amount_per_cycle,
        cycle_frequency,
        None,
        None,
        start_at,
    )?;
    msg!("DCA setup completed successfully!");

    Ok(())
}
