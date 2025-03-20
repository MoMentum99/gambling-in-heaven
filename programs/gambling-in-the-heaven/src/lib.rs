use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use std::mem::size_of;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

#[program]
pub mod coin_flip {
    use super::*;

    pub fn initialize_house(
        ctx: Context<InitializeHouse>,
        bump: u8,
    ) -> Result<()> {
        let house = &mut ctx.accounts.house;
        house.bump = bump;
        house.authority = ctx.accounts.authority.key();
        house.house_token_account = ctx.accounts.house_token_account.key();
        house.win_count = 0;
        house.loss_count = 0;
        Ok(())
    }

    pub fn deposit_house(
        ctx: Context<DepositHouse>,
        amount: u64,
    ) -> Result<()> {
        let cpi_accounts = Transfer {
            from: ctx.accounts.authority_token_account.to_account_info(),
            to: ctx.accounts.house_token_account.to_account_info(),
            authority: ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        Ok(())
    }

    pub fn withdraw_house(
        ctx: Context<WithdrawHouse>,
        amount: u64,
    ) -> Result<()> {
        let seeds = &[
            b"house".as_ref(),
            &[ctx.accounts.house.bump],
        ];
        let signer = &[&seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.house_token_account.to_account_info(),
            to: ctx.accounts.authority_token_account.to_account_info(),
            authority: ctx.accounts.house.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;
        Ok(())
    }

    pub fn place_bet(
        ctx: Context<PlaceBet>,
        user_seed: u64,
        bet_amount: u64,
        user_guess: bool, // true = heads, false = tails
    ) -> Result<()> {
        // Ensure bet amount is valid
        require!(bet_amount > 0, ErrorCode::InvalidBetAmount);

        // Check house has enough balance
        require!(
            ctx.accounts.house_token_account.amount >= bet_amount,
            ErrorCode::InsufficientHouseBalance
        );

        // Transfer tokens from user to escrow
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, bet_amount)?;

        // Initialize bet account
        let bet = &mut ctx.accounts.bet;
        bet.user = ctx.accounts.user.key();
        bet.house = ctx.accounts.house.key();
        bet.amount = bet_amount;
        bet.user_guess = user_guess;
        bet.settled = false;
        bet.escrow_token_account = ctx.accounts.escrow_token_account.key();
        bet.user_seed = user_seed;
        bet.bump = ctx.bumps.bet;

        Ok(())
    }

    pub fn settle_bet(
        ctx: Context<SettleBet>,
        house_seed: u64,
    ) -> Result<()> {
        let bet = &mut ctx.accounts.bet;
        let house = &mut ctx.accounts.house;

        // Ensure bet hasn't been settled already
        require!(!bet.settled, ErrorCode::BetAlreadySettled);

        // Generate random result based on user seed and house seed
        let user_seed = bet.user_seed;
        let result_seed = user_seed.wrapping_add(house_seed);

        // Determine if the result is heads (true) or tails (false)
        let result = (result_seed % 2) == 0;

        // Check if user won
        let user_won = result == bet.user_guess;

        // Mark bet as settled
        bet.settled = true;
        bet.house_seed = house_seed;
        bet.result = result;

        // Update house statistics
        if user_won {
            house.loss_count = house.loss_count.checked_add(1).unwrap();
        } else {
            house.win_count = house.win_count.checked_add(1).unwrap();
        }

        // Transfer tokens based on result
        let seeds = &[
            b"bet".as_ref(),
            bet.user.as_ref(),
            &bet.user_seed.to_le_bytes(),
            &[bet.bump],
        ];
        let signer = &[&seeds[..]];

        if user_won {
            // User won, transfer double the bet amount from house to user
            // win_amount 계산 (사용처에서 직접 bet.amount를 사용)

            // Transfer bet amount from escrow to user
            let escrow_cpi_accounts = Transfer {
                from: ctx.accounts.escrow_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.bet.to_account_info(),
            };
            let token_program_info = ctx.accounts.token_program.to_account_info();
            let escrow_cpi_ctx = CpiContext::new_with_signer(
                token_program_info.clone(),
                escrow_cpi_accounts,
                signer
            );
            token::transfer(escrow_cpi_ctx, bet.amount)?;

            // Transfer additional win amount from house to user
            let house_seeds = &[
                b"house".as_ref(),
                &[house.bump],
            ];
            let house_signer = &[&house_seeds[..]];

            let house_cpi_accounts = Transfer {
                from: ctx.accounts.house_token_account.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.house.to_account_info(),
            };
            let house_cpi_ctx = CpiContext::new_with_signer(
                token_program_info,
                house_cpi_accounts,
                house_signer
            );
            token::transfer(house_cpi_ctx, bet.amount)?;
        } else {
            // House won, transfer bet amount from escrow to house
            let cpi_accounts = Transfer {
                from: ctx.accounts.escrow_token_account.to_account_info(),
                to: ctx.accounts.house_token_account.to_account_info(),
                authority: ctx.accounts.bet.to_account_info(),
            };
            let token_program_info = ctx.accounts.token_program.to_account_info();
            let cpi_ctx = CpiContext::new_with_signer(token_program_info, cpi_accounts, signer);
            token::transfer(cpi_ctx, bet.amount)?;
        }

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(bump: u8)]
pub struct InitializeHouse<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + size_of::<House>(),
        seeds = [b"house".as_ref()],
        bump
    )]
    pub house: Account<'info, House>,

    #[account(mut)]
    pub house_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositHouse<'info> {
    #[account(
        mut,
        seeds = [b"house".as_ref()],
        bump = house.bump,
        has_one = authority,
        has_one = house_token_account
    )]
    pub house: Account<'info, House>,

    #[account(mut)]
    pub house_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mut)]
    pub authority_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct WithdrawHouse<'info> {
    #[account(
        mut,
        seeds = [b"house".as_ref()],
        bump = house.bump,
        has_one = authority,
        has_one = house_token_account
    )]
    pub house: Account<'info, House>,

    #[account(mut)]
    pub house_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mut)]
    pub authority_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(user_seed: u64, bet_amount: u64, user_guess: bool)]
pub struct PlaceBet<'info> {
    #[account(
        init,
        payer = user,
        space = 8 + size_of::<Bet>(),
        seeds = [
            b"bet".as_ref(),
            user.key().as_ref(),
            &user_seed.to_le_bytes(),
        ],
        bump
    )]
    pub bet: Account<'info, Bet>,

    #[account(
        seeds = [b"house".as_ref()],
        bump = house.bump
    )]
    pub house: Account<'info, House>,

    #[account(mut)]
    pub user: Signer<'info>,

    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub house_token_account: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = user,
        token::mint = token_mint,
        token::authority = bet,
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,

    pub token_mint: Account<'info, token::Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(house_seed: u64)]
pub struct SettleBet<'info> {
    #[account(
        mut,
        seeds = [
            b"bet".as_ref(),
            bet.user.as_ref(),
            &bet.user_seed.to_le_bytes(),
        ],
        bump = bet.bump,
        has_one = user,
        has_one = house,
        has_one = escrow_token_account,
    )]
    pub bet: Account<'info, Bet>,

    #[account(
        mut,
        seeds = [b"house".as_ref()],
        bump = house.bump,
        has_one = house_token_account
    )]
    pub house: Account<'info, House>,

    #[account(mut)]
    pub user: AccountInfo<'info>,

    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub house_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub escrow_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct House {
    pub bump: u8,
    pub authority: Pubkey,
    pub house_token_account: Pubkey,
    pub win_count: u64,
    pub loss_count: u64,
}

#[account]
pub struct Bet {
    pub user: Pubkey,
    pub house: Pubkey,
    pub amount: u64,
    pub user_guess: bool, // true = heads, false = tails
    pub user_seed: u64,
    pub house_seed: u64,
    pub result: bool,
    pub settled: bool,
    pub escrow_token_account: Pubkey,
    pub bump: u8,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Invalid bet amount")]
    InvalidBetAmount,
    #[msg("Insufficient house balance")]
    InsufficientHouseBalance,
    #[msg("Bet already settled")]
    BetAlreadySettled,
}