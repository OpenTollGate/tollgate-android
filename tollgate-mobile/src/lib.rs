//! tollgate-mobile — Native Android TollGate client core.
//!
//! Rust core exposed to Kotlin via UniFFI. Handles identity (keypair),
//! Cashu wallet (CDK), and TollGate protocol client logic.
//!
//! Architecture: Rust → UniFFI → Kotlin/Jetpack Compose.
//! Modeled on Myco (Origami74) and fips-android (Michael Malmi).

uniffi::setup_scaffolding!("tollgate_mobile");

pub mod wallet;

use std::path::Path;
use std::sync::Mutex;

use secp256k1::rand::RngCore;
use secp256k1::{Secp256k1, SecretKey, PublicKey};

use crate::wallet::CashuWallet;

// ---------------------------------------------------------------------------
// UniFFI-compatible error
// ---------------------------------------------------------------------------

#[derive(Debug, uniffi::Error, thiserror::Error)]
pub enum TollgateError {
    #[error("wallet not initialized")]
    WalletNotInitialized,
    #[error("operation failed: {msg}")]
    OperationFailed { msg: String },
}

impl From<anyhow::Error> for TollgateError {
    fn from(e: anyhow::Error) -> Self {
        TollgateError::OperationFailed { msg: e.to_string() }
    }
}

// ---------------------------------------------------------------------------
// UniFFI-exposed types
// ---------------------------------------------------------------------------

/// Wallet state exposed to UI.
#[derive(uniffi::Record, Clone, Debug)]
pub struct WalletState {
    pub balance_sat: u64,
    pub mint_url: String,
}

// ---------------------------------------------------------------------------
// TollgateMobileNode — the main UniFFI object
// ---------------------------------------------------------------------------

struct Inner {
    secret_key: SecretKey,
    wallet: Option<CashuWallet>,
    rt: tokio::runtime::Runtime,
    mint_url: String,
}

#[derive(uniffi::Object)]
pub struct TollgateMobileNode {
    inner: Mutex<Inner>,
}

#[uniffi::export]
impl TollgateMobileNode {
    /// Create a new node with identity loaded from (or generated into) `data_dir`.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Self, TollgateError> {
        let dir = Path::new(&data_dir);
        std::fs::create_dir_all(dir).map_err(|e| TollgateError::OperationFailed {
            msg: format!("creating data dir: {e}"),
        })?;

        let key_path = dir.join("secret_key.bin");
        let secret_key = if key_path.exists() {
            let bytes = std::fs::read(&key_path).map_err(|e| TollgateError::OperationFailed {
                msg: format!("reading key: {e}"),
            })?;
            SecretKey::from_slice(&bytes).map_err(|e| TollgateError::OperationFailed {
                msg: format!("parsing key: {e}"),
            })?
        } else {
            let mut seed = [0u8; 32];
            secp256k1::rand::rngs::OsRng.fill_bytes(&mut seed);
            let sk = SecretKey::from_slice(&seed).map_err(|e| TollgateError::OperationFailed {
                msg: format!("generating key: {e}"),
            })?;
            // Persist
            std::fs::write(&key_path, &sk[..]).map_err(|e| TollgateError::OperationFailed {
                msg: format!("saving key: {e}"),
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
            }
            sk
        };

        let rt = tokio::runtime::Runtime::new().map_err(|e| TollgateError::OperationFailed {
            msg: format!("creating runtime: {e}"),
        })?;

        Ok(Self {
            inner: Mutex::new(Inner {
                secret_key,
                wallet: None,
                rt,
                mint_url: crate::wallet::DEFAULT_MINT.to_string(),
            }),
        })
    }

    /// Hex pubkey (33-byte compressed, 66 hex chars).
    pub fn pubkey_hex(&self) -> Result<String, TollgateError> {
        let inner = self.inner.lock().map_err(|e| TollgateError::OperationFailed {
            msg: format!("lock: {e}"),
        })?;
        let secp = Secp256k1::new();
        let pk = PublicKey::from_secret_key(&secp, &inner.secret_key);
        Ok(hex::encode(pk.serialize()))
    }

    /// Set the Cashu mint URL (default: testnut.cashu.space).
    pub fn set_mint(&self, mint_url: String) -> Result<(), TollgateError> {
        let mut inner = self.inner.lock().map_err(|e| TollgateError::OperationFailed {
            msg: format!("lock: {e}"),
        })?;
        inner.mint_url = mint_url;
        inner.wallet = None; // recreate on next use
        Ok(())
    }

    /// Initialize wallet (lazy — call before wallet operations).
    pub fn init_wallet(&self, data_dir: String) -> Result<(), TollgateError> {
        let mut inner = self.inner.lock().map_err(|e| TollgateError::OperationFailed {
            msg: format!("lock: {e}"),
        })?;
        if inner.wallet.is_some() {
            return Ok(());
        }
        let wallet = inner.rt.block_on(CashuWallet::load_or_create(
            Path::new(&data_dir),
            &inner.mint_url,
        ))?;
        inner.wallet = Some(wallet);
        Ok(())
    }

    // ── wallet methods (synchronous — block on internal runtime) ──────────

    /// Get wallet balance in sats.
    pub fn wallet_balance(&self) -> Result<u64, TollgateError> {
        let inner = self.inner.lock().map_err(|_| TollgateError::WalletNotInitialized)?;
        let w = inner.wallet.as_ref().ok_or(TollgateError::WalletNotInitialized)?;
        let bal = inner.rt.block_on(w.balance())?;
        Ok(bal)
    }

    /// Mint sats from the configured mint. For test mints this auto-settles.
    pub fn wallet_mint(&self, amount_sat: u64) -> Result<u64, TollgateError> {
        let inner = self.inner.lock().map_err(|_| TollgateError::WalletNotInitialized)?;
        let w = inner.wallet.as_ref().ok_or(TollgateError::WalletNotInitialized)?;
        let bal = inner.rt.block_on(w.mint(amount_sat))?;
        Ok(bal)
    }

    /// Send sats as a Cashu token string.
    pub fn wallet_send(&self, amount_sat: u64) -> Result<String, TollgateError> {
        let inner = self.inner.lock().map_err(|_| TollgateError::WalletNotInitialized)?;
        let w = inner.wallet.as_ref().ok_or(TollgateError::WalletNotInitialized)?;
        let token = inner.rt.block_on(w.send_token(amount_sat))?;
        Ok(token)
    }

    /// Receive a Cashu token string.
    pub fn wallet_receive(&self, token_str: String) -> Result<u64, TollgateError> {
        let inner = self.inner.lock().map_err(|_| TollgateError::WalletNotInitialized)?;
        let w = inner.wallet.as_ref().ok_or(TollgateError::WalletNotInitialized)?;
        let bal = inner.rt.block_on(w.receive(&token_str))?;
        Ok(bal)
    }

    /// Get wallet state for the UI.
    pub fn wallet_state(&self) -> Result<WalletState, TollgateError> {
        let inner = self.inner.lock().map_err(|_| TollgateError::WalletNotInitialized)?;
        let balance = match inner.wallet.as_ref() {
            Some(w) => inner.rt.block_on(w.balance()).unwrap_or(0),
            None => 0,
        };
        Ok(WalletState {
            balance_sat: balance,
            mint_url: inner.mint_url.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let node = TollgateMobileNode::new(tmp.path().to_string_lossy().to_string()).unwrap();
        let pk = node.pubkey_hex().unwrap();
        assert_eq!(pk.len(), 66, "pubkey should be 66 hex chars (33 bytes compressed)");
        // Loading again from same dir should yield same key
        let node2 = TollgateMobileNode::new(tmp.path().to_string_lossy().to_string()).unwrap();
        assert_eq!(node2.pubkey_hex().unwrap(), pk, "identity should be persistent");
    }

    #[test]
    fn test_init_wallet() {
        let tmp = tempfile::tempdir().unwrap();
        let node = TollgateMobileNode::new(tmp.path().to_string_lossy().to_string()).unwrap();
        // Should be able to init wallet without error (local store + seed creation)
        assert!(node.init_wallet(tmp.path().to_string_lossy().to_string()).is_ok());
        // Second init should be a no-op
        assert!(node.init_wallet(tmp.path().to_string_lossy().to_string()).is_ok());
    }
}
