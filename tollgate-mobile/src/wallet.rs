//! Real Cashu wallet using CDK.
//!
//! Replaces the fake `build_bootstrap_token` stub with a wallet that actually
//! mints, stores, and spends ecash from a configured mint (e.g.
//! `testnut.cashu.space` for testing).
//!
//! Uses CDK's `Wallet` type with `cdk-redb` for persistent on-device storage.
//! The wallet seed is persisted to `<data_dir>/wallet_seed.bin` (64 bytes, 0600).

use std::path::Path;

use anyhow::{anyhow, Context};
use cdk::amount::Amount;
use cdk::amount::SplitTarget;
use cdk::nuts::{CurrencyUnit, MintQuoteState, PaymentMethod};
use cdk::wallet::{ReceiveOptions, SendOptions, Wallet};
use cdk_redb::WalletRedbDatabase;

/// Default test mint. Can be overridden by the app via `set_mint`.
pub const DEFAULT_MINT: &str = "https://testnut.cashu.space";

/// Wraps a CDK `Wallet` with persistent storage + seed management.
///
/// The wallet is created lazily on first use (or on `new`) and persists:
/// - `<data_dir>/wallet_seed.bin` — 64-byte random seed (owns the ecash)
/// - `<data_dir>/wallet.redb` — CDK redb database (proofs, keysets, quotes)
pub struct CashuWallet {
    wallet: Wallet,
}

impl CashuWallet {
    /// Load-or-create the wallet under `data_dir` using `mint_url`.
    /// Generates + persists a random seed if none exists.
    pub async fn load_or_create(
        data_dir: &Path,
        mint_url: &str,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir).context("creating wallet data dir")?;

        // Load or generate seed (64 bytes — owns all ecash, NEVER lose this)
        let seed_path = data_dir.join("wallet_seed.bin");
        let seed = match std::fs::read(&seed_path) {
            Ok(bytes) if bytes.len() == 64 => {
                let mut s = [0u8; 64];
                s.copy_from_slice(&bytes);
                s
            }
            _ => {
                let mut s = [0u8; 64];
                // Use secp256k1's rand wrapper (already a dep, works on Android)
                use secp256k1::rand::RngCore;
                secp256k1::rand::rngs::OsRng.fill_bytes(&mut s);
                // Persist immediately (best-effort perms)
                let _ = std::fs::write(&seed_path, s);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &seed_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
                s
            }
        };

        // Open or create the redb database
        let db_path = data_dir.join("wallet.redb");
        let localstore = std::sync::Arc::new(
            WalletRedbDatabase::new(&db_path)
                .map_err(|e| anyhow!("opening wallet database: {e}"))?,
        );

        let wallet = Wallet::new(
            mint_url,
            CurrencyUnit::Sat,
            localstore,
            seed,
            None, // target_proof_count = default (3)
        )
        .map_err(|e| anyhow!("creating CDK wallet: {e}"))?;

        Ok(Self { wallet })
    }

    /// Current total balance in sats (sum of unspent proofs).
    pub async fn balance(&self) -> anyhow::Result<u64> {
        let amount = self
            .wallet
            .total_balance()
            .await
            .map_err(|e| anyhow!("fetching balance: {e}"))?;
        Ok(amount.into())
    }

    /// Mint `amount_sat` from the configured mint.
    ///
    /// For test mints like testnut.cashu.space, the Lightning quote auto-settles
    /// (no real payment needed). For production mints, the user would need to
    /// pay the Lightning invoice first.
    ///
    /// Returns the new balance after minting.
    pub async fn mint(&self, amount_sat: u64) -> anyhow::Result<u64> {
        // 1. Request a mint quote (NUT-04)
        let quote = self
            .wallet
            .mint_quote(
                PaymentMethod::BOLT11,
                Some(Amount::from(amount_sat)),
                None,
                None,
            )
            .await
            .map_err(|e| anyhow!("mint quote: {e}"))?;

        // 2. For test mints, poll until PAID (auto-settles immediately).
        //    For real mints, user would need to pay the invoice from `quote.request`.
        //    We poll for up to 30 seconds.
        let mut state = quote.state.clone();
        for _ in 0..30 {
            if matches!(state, MintQuoteState::Paid | MintQuoteState::Issued) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let updated = self
                .wallet
                .check_mint_quote_status(&quote.id)
                .await
                .map_err(|e| anyhow!("checking quote status: {e}"))?;
            state = updated.state;
        }

        if !matches!(state, MintQuoteState::Paid | MintQuoteState::Issued) {
            return Err(anyhow!(
                "mint quote not paid after 30s — for real mints, pay the Lightning invoice first: {}",
                quote.request
            ));
        }

        // 3. Mint the tokens (NUT-04 final step — unblind signatures)
        let _proofs = self
            .wallet
            .mint(&quote.id, SplitTarget::default(), None)
            .await
            .map_err(|e| anyhow!("minting tokens: {e}"))?;

        // 4. Return new balance
        self.balance().await
    }

    /// Send `amount_sat` as a Cashu token string (cashuB...).
    ///
    /// This swaps proofs at the mint to produce exact-change tokens that can be
    /// sent to a gateway. The gateway verifies them via NUT-07 checkstate.
    pub async fn send_token(&self, amount_sat: u64) -> anyhow::Result<String> {
        let prepared = self
            .wallet
            .prepare_send(Amount::from(amount_sat), SendOptions::default())
            .await
            .map_err(|e| anyhow!("preparing send: {e}"))?;

        let token = prepared
            .confirm(None)
            .await
            .map_err(|e| anyhow!("confirming send: {e}"))?;

        Ok(token.to_string())
    }

    /// Receive a Cashu token string, adding its proofs to the wallet.
    /// Returns the new balance after receiving.
    pub async fn receive(&self, token_str: &str) -> anyhow::Result<u64> {
        let opts = ReceiveOptions {
            amount_split_target: SplitTarget::default(),
            p2pk_signing_keys: vec![],
            preimages: vec![],
            metadata: std::collections::HashMap::new(),
        };
        let _amount = self
            .wallet
            .receive(token_str, opts)
            .await
            .map_err(|e| anyhow!("receiving token: {e}"))?;
        self.balance().await
    }

    /// Get the Lightning invoice for minting (useful for real mints).
    /// Returns the request string from the mint quote.
    pub async fn get_mint_invoice(&self, amount_sat: u64) -> anyhow::Result<String> {
        let quote = self
            .wallet
            .mint_quote(
                PaymentMethod::BOLT11,
                Some(Amount::from(amount_sat)),
                None,
                None,
            )
            .await
            .map_err(|e| anyhow!("mint quote: {e}"))?;
        Ok(quote.request)
    }
}
