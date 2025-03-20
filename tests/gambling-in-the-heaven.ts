import * as anchor from '@project-serum/anchor';
import { Program } from '@project-serum/anchor';
import { CoinFlip } from '../target/types/gambling-in-the-heaven';
import { Token, TOKEN_PROGRAM_ID } from '@solana/spl-token';
import { assert } from 'chai';

describe('gambling-in-the-heaven', () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.CoinFlip as Program<CoinFlip>;

  // We'll need a mint for testing
  let mint: Token;
  let userTokenAccount: anchor.web3.PublicKey;
  let houseTokenAccount: anchor.web3.PublicKey;

  // House account
  const house = anchor.web3.Keypair.generate();
  const housePDA = anchor.web3.PublicKey.findProgramAddressSync(
      [Buffer.from("house")],
      program.programId
  );
  const houseAddress = housePDA[0];
  const houseBump = housePDA[1];

  // Test setup
  before(async () => {
    // Create a new mint
    mint = await Token.createMint(
        provider.connection,
        provider.wallet.payer,
        provider.wallet.publicKey,
        null,
        6,
        TOKEN_PROGRAM_ID
    );

    // Create token accounts for user and house
    userTokenAccount = await mint.createAccount(provider.wallet.publicKey);
    houseTokenAccount = await mint.createAccount(houseAddress);

    // Mint some tokens to user for testing
    await mint.mintTo(
        userTokenAccount,
        provider.wallet.publicKey,
        [],
        1_000_000_000
    );
  });

  it('Initialize house', async () => {
    await program.methods
        .initializeHouse(houseBump)
        .accounts({
          house: houseAddress,
          houseTokenAccount: houseTokenAccount,
          authority: provider.wallet.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .rpc();

    const houseAccount = await program.account.house.fetch(houseAddress);
    assert.equal(houseAccount.authority.toString(), provider.wallet.publicKey.toString());
    assert.equal(houseAccount.houseTokenAccount.toString(), houseTokenAccount.toString());
    assert.equal(houseAccount.winCount.toString(), '0');
    assert.equal(houseAccount.lossCount.toString(), '0');
  });

  it('Deposit to house', async () => {
    const depositAmount = new anchor.BN(500_000_000);

    await program.methods
        .depositHouse(depositAmount)
        .accounts({
          house: houseAddress,
          houseTokenAccount: houseTokenAccount,
          authority: provider.wallet.publicKey,
          authorityTokenAccount: userTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();

    const houseTokenBalance = await mint.getAccountInfo(houseTokenAccount);
    assert.equal(houseTokenBalance.amount.toString(), depositAmount.toString());
  });

  it('Place and settle a bet', async () => {
    const userSeed = new anchor.BN(Math.floor(Math.random() * 1_000_000));
    const betAmount = new anchor.BN(100_000_000);
    const userGuess = true; // heads

    // Calculate PDA for the bet account
    const betPDA = anchor.web3.PublicKey.findProgramAddressSync(
        [
          Buffer.from("bet"),
          provider.wallet.publicKey.toBuffer(),
          userSeed.toArrayLike(Buffer, 'le', 8),
        ],
        program.programId
    );
    const betAddress = betPDA[0];

    // Create an escrow token account
    const escrowTokenAccount = await mint.createAccount(betAddress);

    // Place the bet
    await program.methods
        .placeBet(userSeed, betAmount, userGuess)
        .accounts({
          bet: betAddress,
          house: houseAddress,
          user: provider.wallet.publicKey,
          userTokenAccount: userTokenAccount,
          houseTokenAccount: houseTokenAccount,
          escrowTokenAccount: escrowTokenAccount,
          tokenMint: mint.publicKey,
          tokenProgram: TOKEN_PROGRAM_ID,
          systemProgram: anchor.web3.SystemProgram.programId,
          rent: anchor.web3.SYSVAR_RENT_PUBKEY,
        })
        .rpc();

    // Verify bet was placed correctly
    const betAccount = await program.account.bet.fetch(betAddress);
    assert.equal(betAccount.user.toString(), provider.wallet.publicKey.toString());
    assert.equal(betAccount.house.toString(), houseAddress.toString());
    assert.equal(betAccount.amount.toString(), betAmount.toString());
    assert.equal(betAccount.userGuess, userGuess);
    assert.equal(betAccount.settled, false);
    assert.equal(betAccount.escrowTokenAccount.toString(), escrowTokenAccount.toString());

    // Get balances before settlement
    const userBalanceBefore = await mint.getAccountInfo(userTokenAccount);
    const houseBalanceBefore = await mint.getAccountInfo(houseTokenAccount);

    // Settle the bet with a house seed
    const houseSeed = new anchor.BN(Math.floor(Math.random() * 1_000_000));

    await program.methods
        .settleBet(houseSeed)
        .accounts({
          bet: betAddress,
          house: houseAddress,
          user: provider.wallet.publicKey,
          userTokenAccount: userTokenAccount,
          houseTokenAccount: houseTokenAccount,
          escrowTokenAccount: escrowTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();

    // Verify bet was settled
    const settledBetAccount = await program.account.bet.fetch(betAddress);
    assert.equal(settledBetAccount.settled, true);
    assert.equal(settledBetAccount.houseSeed.toString(), houseSeed.toString());

    // Get balances after settlement
    const userBalanceAfter = await mint.getAccountInfo(userTokenAccount);
    const houseBalanceAfter = await mint.getAccountInfo(houseTokenAccount);

    // Check house statistics
    const houseAccount = await program.account.house.fetch(houseAddress);

    console.log("Result:", settledBetAccount.result);
    console.log("User guess:", userGuess);
    console.log("User won:", settledBetAccount.result === userGuess);
    console.log("House win count:", houseAccount.winCount.toString());
    console.log("House loss count:", houseAccount.lossCount.toString());

    // Verify balances changed appropriately based on result
    if (settledBetAccount.result === userGuess) {
      // User won
      assert.equal(
          userBalanceAfter.amount.toString(),
          userBalanceBefore.amount.add(betAmount).toString(),
          "User should have received winnings"
      );
      assert.equal(
          houseBalanceAfter.amount.toString(),
          houseBalanceBefore.amount.sub(betAmount).toString(),
          "House should have paid out winnings"
      );
    } else {
      // House won
      assert.equal(
          houseBalanceAfter.amount.toString(),
          houseBalanceBefore.amount.add(betAmount).toString(),
          "House should have received bet amount"
      );
    }
  });

  it('Withdraw from house', async () => {
    // Get current house balance
    const houseBalanceBefore = await mint.getAccountInfo(houseTokenAccount);
    const withdrawAmount = new anchor.BN(houseBalanceBefore.amount.toString());

    await program.methods
        .withdrawHouse(withdrawAmount)
        .accounts({
          house: houseAddress,
          houseTokenAccount: houseTokenAccount,
          authority: provider.wallet.publicKey,
          authorityTokenAccount: userTokenAccount,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();

    // Verify withdrawal
    const houseBalanceAfter = await mint.getAccountInfo(houseTokenAccount);
    assert.equal(houseBalanceAfter.amount.toString(), '0', "House balance should be 0 after full withdrawal");
  });
});