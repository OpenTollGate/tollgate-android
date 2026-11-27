//! The ecash this device holds, and how it spends it buying access.
//!
//! A thin layer over cdk's wallet, ported from `tollgate-net`'s `wallet.rs`
//! rather than reinvented: proof selection, change, keyset rotation and the
//! spent-proof bookkeeping are Cashu problems cdk has already solved, and a
//! second implementation of them would be a second set of bugs.
//!
//! # Why the phone wants a wallet at all
//!
//! Buying v1 access does **not** need one. The merchant opens a NUT-04 quote at
//! its own mint, the buyer pays a BOLT11 invoice from whatever Lightning wallet
//! they already have, and the vouchers are issued to the merchant. That is the
//! default path and it stays the default.
//!
//! A wallet earns its place for the two things an invoice cannot do:
//!
//! 1. **Paying with paper already held.** Users accumulate ecash — change from
//!    a previous purchase, a tip, a grant. Forcing them to a Lightning invoice
//!    when they are holding the money is friction for its own sake, and it costs
//!    them a second payment channel they may not have.
//! 2. **Resale.** Buying from an upstream to sell downstream needs somewhere to
//!    hold value between the two, which is a wallet by definition.
//!
//! # One wallet per (mint, unit)
//!
//! Paper from one issuer is not paper from another, and sat-denominated tokens
//! are not byte-denominated ones. Mixing them would be minting. cdk takes a
//! `CurrencyUnit` per wallet and the seed is fixed, so the map here is a routing
//! table, not a convenience.
//!
//! # The seed is the device identity
//!
//! Derived from the node's own secret, so the wallet is part of the identity
//! rather than a separate thing to back up: restore the config on a new phone
//! and the proofs are recoverable from it. That also means losing the identity
//! loses the money, which is why [`Wallet::open`] never creates a seed it was not
//! handed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cdk::amount::SplitTarget;
use cdk::nuts::{CurrencyUnit, Token};
use cdk::wallet::{ReceiveOptions, SendOptions, Wallet as CdkWallet};
use cdk_common::database::{self, WalletDatabase};
use cdk_sqlite::wallet::WalletSqliteDatabase;
use tokio::sync::Mutex;

/// What can go wrong holding or spending ecash.
#[derive(Debug, thiserror::Error)]
pub enum WalletError {
    /// The database could not be opened, or a query against it failed.
    #[error("wallet storage: {0}")]
    Storage(String),

    /// The token is not a Cashu token, or names a unit this build cannot route.
    #[error("that is not a token this wallet can read: {0}")]
    NotAToken(String),

    /// The mint refused, or could not be reached.
    #[error("{mint}: {message}")]
    Mint {
        /// The issuer that refused.
        mint: String,
        /// What it said.
        message: String,
    },

    /// There is not enough of that (mint, unit) to pay this.
    #[error("this device holds {held} {unit} from {mint}, and {asked} was asked for")]
    Short {
        /// What the issuer is.
        mint: String,
        /// Which unit.
        unit: String,
        /// What is held.
        held: u64,
        /// What was wanted.
        asked: u64,
    },

    /// Spending zero is not a payment.
    #[error("spending nothing is not a payment")]
    NothingAsked,
}

/// A balance in one (mint, unit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    /// The issuer.
    pub mint: String,
    /// The unit, as the mint spells it (`sat`, `usd`, `bytes`, …).
    pub unit: String,
    /// How much is held.
    pub amount: u64,
}

/// The proofs on this device, one cdk wallet per (mint, unit).
pub struct Wallet {
    /// Lazily-opened wallets, keyed by (normalised mint, unit).
    wallets: Mutex<BTreeMap<(String, String), CdkWallet>>,
    /// The one database all of them share.
    store: Arc<dyn WalletDatabase<database::Error> + Send + Sync>,
    /// The device seed. See the module docs: this is the identity, not a backup.
    seed: [u8; 64],
    /// Where the database lives, for anything that has to say so.
    path: PathBuf,
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet").field("path", &self.path).finish()
    }
}

impl Wallet {
    /// Open the wallet database at `path`, creating it if it is not there.
    ///
    /// The caller supplies the seed; see the module docs for why this function
    /// will not invent one.
    pub async fn open(path: impl Into<PathBuf>, seed: [u8; 64]) -> Result<Self, WalletError> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| WalletError::Storage(format!("create {}: {e}", dir.display())))?;
            }
        }
        let database = WalletSqliteDatabase::new(path.to_string_lossy().as_ref())
            .await
            .map_err(|e| WalletError::Storage(format!("open {}: {e}", path.display())))?;
        // `Wallet::new` takes a trait object; keep the concrete handle too, because
        // it is what the per-(mint, unit) wallets are all built on.
        let store: Arc<dyn WalletDatabase<database::Error> + Send + Sync> = Arc::new(database);
        Ok(Self {
            wallets: Mutex::new(BTreeMap::new()),
            store,
            seed,
            path,
        })
    }

    /// Where the database lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The cdk wallet for one issuer's paper, made on first use.
    async fn wallet_for(&self, mint: &str, unit: &str) -> Result<CdkWallet, WalletError> {
        let key = (normalise(mint), unit.to_string());
        let mut wallets = self.wallets.lock().await;
        if let Some(wallet) = wallets.get(&key) {
            return Ok(wallet.clone());
        }
        let wallet = CdkWallet::new(
            &key.0,
            currency_unit(unit),
            Arc::clone(&self.store),
            self.seed,
            None,
        )
        .map_err(|e| WalletError::Mint {
            mint: key.0.clone(),
            message: format!("cannot open a wallet for {unit}: {e}"),
        })?;
        wallets.insert(key, wallet.clone());
        Ok(wallet)
    }

    /// Take a token in and hold it.
    ///
    /// Receiving is a swap at the issuing mint, so this both proves the paper was
    /// good and moves it under keys only this device has. The amount returned is
    /// what the mint said it was worth — which is the number a v1 purchase is
    /// priced against, not the number the advertisement implied.
    pub async fn receive(&self, token: &str) -> Result<Holding, WalletError> {
        let (mint, unit) = describe(token)?;
        let wallet = self.wallet_for(&mint, &unit).await?;
        let amount: u64 = wallet
            .receive(token.trim(), ReceiveOptions::default())
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.clone(),
                message: format!("receive {unit}: {e}"),
            })?
            .into();
        Ok(Holding { mint, unit, amount })
    }

    /// Take exactly `amount` of one issuer's paper out, as a token.
    ///
    /// cdk chooses which proofs go and swaps for change where it must; what comes
    /// back is a token for exactly `amount`, or an error and an untouched balance.
    pub async fn spend(&self, mint: &str, unit: &str, amount: u64) -> Result<String, WalletError> {
        if amount == 0 {
            return Err(WalletError::NothingAsked);
        }
        let wallet = self.wallet_for(mint, unit).await?;
        let prepared = wallet
            .prepare_send(amount.into(), SendOptions::default())
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.to_string(),
                message: format!("cannot pay {amount} {unit}: {e}"),
            })?;
        let token = prepared
            .confirm(None)
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.to_string(),
                message: format!("send {amount} {unit}: {e}"),
            })?;
        Ok(token.to_string())
    }

    /// Take a token in and hand its value back out as smaller tokens, none worth
    /// more than `most`.
    ///
    /// The use is a purchase split across several sellers, or one seller that only
    /// takes small paper. cdk does the splitting; this only decides the sizes.
    pub async fn split(&self, token: &str, most: u64) -> Result<Vec<String>, WalletError> {
        if most == 0 {
            return Err(WalletError::NothingAsked);
        }
        let (mint, unit) = describe(token)?;
        let held = self.receive(token).await?;
        let wallet = self.wallet_for(&mint, &unit).await?;

        // Pull the proofs for the amount we just received and swap them down into
        // the denominations we want. `swap` is cdk's saga-pattern rewrite of the
        // old `split`: it reserves the inputs so a concurrent spend cannot take
        // them, and hands back the new proofs or nothing.
        let prepared = wallet
            .prepare_send(held.amount.into(), SendOptions::default())
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.clone(),
                message: format!("preparing to split {} {unit}: {e}", held.amount),
            })?;
        let inputs = prepared.proofs().to_vec();
        let _ = prepared.cancel().await;
        let proofs = wallet
            .swap(
                Some(held.amount.into()),
                SplitTarget::default(),
                inputs,
                None,
                true,
                false,
            )
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.clone(),
                message: format!("split {} {unit}: {e}", held.amount),
            })?
            .ok_or_else(|| WalletError::Mint {
                mint: mint.clone(),
                message: "the split produced no proofs".into(),
            })?;

        // Group the proofs into tokens no larger than `most`. A single proof
        // bigger than `most` cannot be split further here without another swap, so
        // it goes out alone rather than being silently dropped.
        let mut out = Vec::new();
        let mut batch: Vec<_> = Vec::new();
        let mut batch_total = 0u64;
        for proof in proofs {
            let value: u64 = proof.amount.into();
            if batch_total + value > most && !batch.is_empty() {
                out.push(
                    self.token_from(&mint, &unit, std::mem::take(&mut batch))
                        .await?,
                );
                batch_total = 0;
            }
            batch_total += value;
            batch.push(proof);
        }
        if !batch.is_empty() {
            out.push(self.token_from(&mint, &unit, batch).await?);
        }
        Ok(out)
    }

    /// Serialize `proofs` as a token for `mint`/`unit`.
    async fn token_from(
        &self,
        mint: &str,
        unit: &str,
        proofs: Vec<cdk::nuts::Proof>,
    ) -> Result<String, WalletError> {
        let mint_url: cdk::mint_url::MintUrl = mint.parse().map_err(|e| WalletError::Mint {
            mint: mint.to_string(),
            message: format!("not a mint URL: {e}"),
        })?;
        let token = Token::new(mint_url, proofs, None, currency_unit(unit));
        Ok(token.to_string())
    }

    /// What this device holds, money first.
    ///
    /// Sorted sat-denominated before the rest because a device short of money
    /// cannot buy from anyone, where one short of a particular issuer's vouchers is
    /// only short with that issuer.
    pub async fn balance(&self) -> Vec<Holding> {
        let wallets = self.wallets.lock().await;
        let mut out = Vec::new();
        for ((mint, unit), wallet) in wallets.iter() {
            let Ok(amount) = wallet.total_balance().await else {
                continue;
            };
            let amount: u64 = amount.into();
            if amount > 0 {
                out.push(Holding {
                    mint: mint.clone(),
                    unit: unit.clone(),
                    amount,
                });
            }
        }
        out.sort_by(|a, b| {
            let money = |h: &Holding| (h.unit != "sat") as u8;
            money(a).cmp(&money(b)).then(a.mint.cmp(&b.mint))
        });
        out
    }

    /// Open a wallet for `mint`/`unit` so its balance shows up in [`Self::balance`]
    /// even before it holds anything. Used at startup for the mints a gateway
    /// advertises.
    pub async fn track(&self, mint: &str, unit: &str) -> Result<(), WalletError> {
        self.wallet_for(mint, unit).await.map(|_| ())
    }

    /// Mint `amount` of `mint`'s paper into this wallet, by quote.
    ///
    /// A NUT-04 round trip: ask the mint for a quote, report its invoice so the
    /// caller can pay it however they pay things, and once the mint says `PAID`,
    /// have cdk blind and unblind the proofs. Returns the quote so a caller can show
    /// the invoice before paying rather than after.
    ///
    /// `auto_accept` mints (the test rig) settle the quote on creation, so this waits
    /// on a state the caller never has to produce itself.
    pub async fn mint(
        &self,
        mint: &str,
        unit: &str,
        amount: u64,
    ) -> Result<MintRequest, WalletError> {
        if amount == 0 {
            return Err(WalletError::NothingAsked);
        }
        let wallet = self.wallet_for(mint, unit).await?;
        let quote = wallet
            .mint_quote(
                cdk::nuts::PaymentMethod::Known(cdk::nuts::nut00::KnownMethod::Bolt11),
                Some(amount.into()),
                // No description: cdk-mintd refuses a quote that carries one when the
                // mint does not advertise `description` support, and a top-up needs
                // none. The quote id is the identity.
                None,
                None,
            )
            .await
            .map_err(|e| WalletError::Mint {
                mint: mint.to_string(),
                message: format!("asking for a {amount} {unit} quote: {e}"),
            })?;

        Ok(MintRequest {
            quote: quote.id.clone(),
            invoice: quote.request.clone(),
            amount,
            mint: mint.to_string(),
            unit: unit.to_string(),
        })
    }

    /// Complete a mint once its quote has been paid.
    ///
    /// Separate from [`Self::mint`] because paying an invoice is the caller's job and
    /// may take minutes; holding a wallet open across it would be a design mistake.
    pub async fn mint_settle(
        &self,
        mint: &str,
        unit: &str,
        quote: &str,
        wait: Duration,
    ) -> Result<u64, WalletError> {
        let wallet = self.wallet_for(mint, unit).await?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            let proofs = wallet
                .mint(quote, SplitTarget::default(), None)
                .await
                .map_err(|e| WalletError::Mint {
                    mint: mint.to_string(),
                    message: format!("minting the proofs for {quote}: {e}"),
                });
            match proofs {
                Ok(proofs) => {
                    let total: u64 = proofs
                        .iter()
                        .map(|p| u64::from(p.amount))
                        .fold(0u64, |a, b| a.saturating_add(b));
                    return Ok(total);
                }
                Err(e) => {
                    // Two outcomes must not be conflated.
                    //
                    // An unpaid quote is the expected case, not a failure: wait it
                    // out, because a Lightning payment may still be settling.
                    //
                    // But a mint that pays quotes on creation (auto_accept) has
                    // already issued the proofs, so asking again is a second claim on
                    // one quote and the mint refuses with "already in use". Retrying
                    // that can never succeed, so report it as what it is instead of
                    // looping until the deadline.
                    let text = e.to_string();
                    if text.contains("already in use") {
                        return Err(WalletError::Mint {
                            mint: mint.to_string(),
                            message: format!(
                                "the mint had already issued this quote's proofs, so this \
                                 wallet did not mint them and does not hold them: {text}"
                            ),
                        });
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(e);
                    }
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            }
        }
    }
}

/// A mint quote the caller can pay.
#[derive(Debug, Clone)]
pub struct MintRequest {
    /// The mint's identifier for this quote.
    pub quote: String,
    /// The BOLT11 invoice to pay. Empty if the mint does not issue one (a
    /// `burn`-method quote does not).
    pub invoice: String,
    /// How much it is for.
    pub amount: u64,
    /// Which issuer.
    pub mint: String,
    /// Which unit.
    pub unit: String,
}

/// The mint and unit a token claims.
///
/// A token with no mint is refused outright: it cannot be redeemed anywhere, so
/// accepting it would be accepting a promise with no promisor. The unit is read
/// from the token's own field, which cdk validates.
fn describe(token: &str) -> Result<(String, String), WalletError> {
    let token: Token = token
        .trim()
        .parse()
        .map_err(|e| WalletError::NotAToken(format!("{e}")))?;
    let mint = token
        .mint_url()
        .map_err(|e| WalletError::NotAToken(format!("the token names no usable mint: {e}")))?;
    let unit = token
        .unit()
        .ok_or_else(|| WalletError::NotAToken("the token names no unit".into()))?;
    Ok((normalise(&mint.to_string()), unit.to_string()))
}

/// A mint URL in the one spelling used as a key.
///
/// Trailing slashes, scheme case and host case are the three spelling differences
/// that have historically produced two balances for one mint. cdk's wallet keys on
/// the URL string it is handed, so this has to agree with itself everywhere: two
/// spellings of one issuer that hash to different keys become two wallets holding
/// halves of the same money.
///
/// The path is left alone. A mint at `https://host/mintA` and one at
/// `https://host/mintB` are different issuers, and folding case there would merge
/// them — the opposite mistake, and the more expensive one.
fn normalise(mint: &str) -> String {
    let trimmed = mint.trim().trim_end_matches('/');
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return trimmed.to_lowercase();
    };
    // Split authority from path; only the authority is case-insensitive.
    let (authority, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    format!(
        "{}://{}{}",
        scheme.to_lowercase(),
        authority.to_lowercase(),
        path
    )
}

/// A unit string as cdk's enum.
///
/// Anything unrecognised becomes `CurrencyUnit::Custom`, which cdk carries
/// through: a gateway selling `bytes` is as legitimate as one selling `sat`, and
/// refusing an unknown unit would make this wallet unable to hold the very
/// vouchers the v1 flow issues.
fn currency_unit(unit: &str) -> CurrencyUnit {
    match unit {
        "sat" => CurrencyUnit::Sat,
        "msat" => CurrencyUnit::Msat,
        "usd" => CurrencyUnit::Usd,
        "eur" => CurrencyUnit::Eur,
        "auth" => CurrencyUnit::Auth,
        other => CurrencyUnit::Custom(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_spellings_agree_on_one_key() {
        assert_eq!(
            normalise("https://Mint.Example/"),
            normalise("https://mint.example")
        );
        assert_eq!(normalise("https://mint.example/"), "https://mint.example");
    }

    #[test]
    fn a_unit_this_build_does_not_know_is_still_a_unit() {
        // A gateway selling bytes must be able to hold its own vouchers.
        assert_eq!(
            currency_unit("bytes"),
            CurrencyUnit::Custom("bytes".to_string())
        );
        assert_eq!(currency_unit("sat"), CurrencyUnit::Sat);
    }

    #[test]
    fn something_that_is_not_a_token_is_refused_as_such() {
        let err = describe("not a cashu token").unwrap_err();
        assert!(matches!(err, WalletError::NotAToken(_)));
    }

    #[test]
    fn spending_nothing_is_not_a_payment() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = std::env::temp_dir().join(format!("wallet-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wallet = rt
            .block_on(Wallet::open(dir.join("w.sqlite"), [7u8; 64]))
            .unwrap();
        let err = rt
            .block_on(wallet.spend("https://mint.example", "sat", 0))
            .unwrap_err();
        assert!(matches!(err, WalletError::NothingAsked));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_wallet_is_empty_and_says_so() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = std::env::temp_dir().join(format!("wallet-empty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let wallet = rt
            .block_on(Wallet::open(dir.join("w.sqlite"), [9u8; 64]))
            .unwrap();
        assert!(rt.block_on(wallet.balance()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
