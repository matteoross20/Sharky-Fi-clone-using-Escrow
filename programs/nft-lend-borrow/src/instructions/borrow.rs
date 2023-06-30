pub use anchor_lang::prelude::*;

use anchor_spl::token::spl_token::instruction::AuthorityType;
use anchor_spl::token::{self, Mint, SetAuthority, Token, TokenAccount, Transfer};

use crate::states::{ActiveLoan, CollectionPool, Offer};

use crate::errors::ErrorCode;

#[derive(Accounts)]
pub struct Borrow<'info> {
    #[account(
        init,
        seeds = [b"active-loan", offer_loan.key().as_ref()],
        bump,
        payer = borrower,
        space = ActiveLoan::LEN
    )]
    pub active_loan: Box<Account<'info, ActiveLoan>>,

    #[account(mut)]
    pub offer_loan: Box<Account<'info, Offer>>,

    /// CHECK: This is safe
    #[account(mut)]
    pub vault_account: AccountInfo<'info>,

    #[account(
        init,
        seeds = [
            b"vault-asset-account",
            offer_loan.key().as_ref(),
        ],
        bump,
        payer = borrower,
        token::mint = asset_mint,
        token::authority = borrower
    )]
    pub vault_asset_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub collection_pool: Box<Account<'info, CollectionPool>>,

    #[account(mut)]
    pub borrower: Signer<'info>,

    #[account(
        mut,
        constraint = borrower_asset_account.owner == *borrower.key,
        constraint = borrower_asset_account.mint == *asset_mint.to_account_info().key
    )]
    pub borrower_asset_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub asset_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,

    pub clock: Sysvar<'info, Clock>,
}

impl<'info> Borrow<'info> {
    fn transfer_to_vault_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.borrower_asset_account.to_account_info().clone(),
            to: self.vault_asset_account.to_account_info().clone(),
            authority: self.borrower.to_account_info().clone(),
        };

        CpiContext::new(self.token_program.to_account_info(), cpi_accounts)
    }

    fn set_authority_context(&self) -> CpiContext<'_, '_, '_, 'info, SetAuthority<'info>> {
        let cpi_accounts = SetAuthority {
            account_or_mint: self.vault_asset_account.to_account_info().clone(),
            current_authority: self.borrower.to_account_info().clone(),
        };

        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

pub fn handler(ctx: Context<Borrow>, minimum_balance_for_rent_exemption: u64) -> Result<()> {
    let active_loan = &mut ctx.accounts.active_loan;
    let offer = &mut ctx.accounts.offer_loan;
    let collection = &mut ctx.accounts.collection_pool;

    if offer.is_loan_taken == true {
        return Err(ErrorCode::LoanAlreadyTaken.into());
    }

    active_loan.collection = collection.key();
    active_loan.offer_account = offer.key();
    active_loan.lender = offer.lender.key();
    active_loan.borrower = ctx.accounts.borrower.key();
    active_loan.mint = ctx.accounts.asset_mint.key();
    active_loan.loan_ts = ctx.accounts.clock.unix_timestamp;
    active_loan.repay_ts = ctx.accounts.clock.unix_timestamp + collection.duration;
    active_loan.is_repaid = false;
    active_loan.is_liquidated = false;
    active_loan.bump = *ctx.bumps.get("active_loan").unwrap();

    offer.borrower = ctx.accounts.borrower.key();
    offer.is_loan_taken = true;

    token::transfer(ctx.accounts.transfer_to_vault_context(), 1)?;

    let vault_lamports_initial = ctx.accounts.vault_account.lamports();

    let transfer_amount = vault_lamports_initial
        .checked_sub(minimum_balance_for_rent_exemption)
        .unwrap();

    token::set_authority(
        ctx.accounts.set_authority_context(),
        AuthorityType::AccountOwner,
        Some(ctx.accounts.vault_account.key()),
    )?;

    **ctx.accounts.vault_account.try_borrow_mut_lamports()? -= transfer_amount;
    **ctx.accounts.borrower.try_borrow_mut_lamports()? += transfer_amount;

    Ok(())
}