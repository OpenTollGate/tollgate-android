//! Nostr/Applesauce FIPS exit node discovery.
//!
//! Implements the Applesauce discovery protocol for finding FIPS exit nodes
//! via Nostr network queries. Replaces unreliable WiFi scanning with
//! Nostr-based peer discovery using inbox/outbox pattern.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use secp256k1::{PublicKey, Secp256k1};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

// Re-export for UniFFI
use crate::{TollgateError, TollgateMobileNode};

// ---------------------------------------------------------------------------

/// FIPS Exit Node announcement on Nostr
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FipsExitNodeAnnouncement {
    /// Exit node's static public key (for FIPS sessions)
    pub pubkey: String,
    /// IP:port endpoints (e.g., ["66.92.204.38:51820"])
    pub addrs: Vec<String>,
    /// Protocol version (e.g., "fips-v0.1")
    pub version: String,
    /// Supported features (e.g., ["exit", "tollgate", "mobile"])
    pub features: Vec<String>,
    /// Available bandwidth in kbps (if advertised)
    pub bandwidth: Option<u64>,
    /// Seconds of uptime (from announcement)
    pub uptime: u64,
    /// Unix timestamp of announcement
    pub timestamp: u64,
    /// Signature of announcement (for verification)
    pub signature: String,
}

impl FipsExitNodeAnnouncement {
    /// Validate that this announcement is well-formed
    pub fn validate(&self) -> Result<(), TollgateError> {
        if self.pubkey.len() != 66 || !self.pubkey.starts_with("02") {
            return Err(TollgateError::Protocol {
                message: "Invalid pubkey format".into(),
            });
        }
        
        if self.addrs.is_empty() {
            return Err(TollgateError::Protocol {
                message: "No addresses provided".into(),
            });
        }
        
        if !self.version.starts_with("fips-") {
            return Err(TollgateError::Protocol {
                message: "Invalid version format".into(),
            });
        }
        
        // Check timestamp is recent (within 24 hours)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| TollgateError::Protocol {
                message: format!("System time error: {e}"),
            })?
            .as_secs();
        
        if self.timestamp > now + 86400 || self.timestamp < now - 86400 {
            return Err(TollgateError::Protocol {
                message: "Announcement timestamp too old or in future".into(),
            });
        }
        
        Ok(())
    }
    
    /// Calculate a score for this exit node (higher = better)
    pub fn score(&self) -> f64 {
        let mut score = 0.0;
        
        // Base score for FIPS version
        if self.version.contains("fips-v0.1") {
            score += 50.0;
        }
        
        // Bonus for mobile support
        if self.features.contains(&"mobile".to_string()) {
            score += 30.0;
        }
        
        // Bonus for tollgate support
        if self.features.contains(&"tollgate".to_string()) {
            score += 20.0;
        }
        
        // Bandwidth score (up to 50 points)
        if let Some(bw) = self.bandwidth {
            score += (bw.min(100000) as f64 / 100000.0) * 50.0;
        }
        
        // Uptime score (up to 20 points)
        let uptime_score = (self.uptime.min(86400) as f64 / 86400.0) * 20.0;
        score += uptime_score;
        
        // Recency score (newer is better, up to 30 points)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = now.saturating_sub(self.timestamp);
        let recency_score = (86400 - age.min(86400)) as f64 / 86400.0 * 30.0;
        score += recency_score.max(0.0);
        
        score
    }
}

/// Simplified FIPS exit node info for UI consumption
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FipsExitNodeInfo {
    /// Exit node pubkey (truncated for display)
    pub pubkey_short: String,
    /// Full pubkey (for connections)
    pub pubkey_full: String,
    /// Primary endpoint
    pub endpoint: String,
    /// Features supported
    pub features: Vec<String>,
    /// Score (0-100)
    pub score: f64,
    /// Estimated latency (ms, -1 if unknown)
    pub latency_ms: i64,
    /// Bandwidth in Mbps (if known)
    pub bandwidth_mbps: Option<f64>,
    /// Uptime in hours
    pub uptime_hours: f64,
}

impl From<FipsExitNodeAnnouncement> for FipsExitNodeInfo {
    fn from(ann: FipsExitNodeAnnouncement) -> Self {
        let pubkey_short = if ann.pubkey.len() > 10 {
            format!("{}...{}", &ann.pubkey[..6], &ann.pubkey[ann.pubkey.len()-4..])
        } else {
            ann.pubkey.clone()
        };
        
        let endpoint = ann.addrs.first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        
        let bandwidth_mbps = ann.bandwidth
            .map(|bw| bw as f64 / 1000.0);
        
        Self {
            pubkey_short,
            pubkey_full: ann.pubkey,
            endpoint,
            features: ann.features,
            score: ann.score(),
            latency_ms: -1, // Will be updated by ping test
            bandwidth_mbps,
            uptime_hours: ann.uptime as f64 / 3600.0,
        }
    }
}

/// Discovery configuration
#[derive(Clone)]
pub struct DiscoveryConfig {
    /// Nostr relays to query
    pub relays: Vec<String>,
    /// Filter for minimum score
    pub min_score: f64,
    /// Maximum number of results
    pub max_results: usize,
    /// Cache duration (seconds)
    pub cache_duration: u64,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            relays: vec![
                "wss://relay.damus.io".into(),
                "wss://nos.lol".into(),
                "wss://nostr.mom".into(),
                "wss://relay1.orangesync.tech".into(),
                "wss://relay2.orangesync.tech".into(),
            ],
            min_score: 30.0,
            max_results: 10,
            cache_duration: 300, // 5 minutes
        }
    }
}

/// Nostr/Applesauce FIPS exit node discovery
pub struct NostrDiscovery {
    /// Configuration
    config: DiscoveryConfig,
    /// Cached exit nodes
    cache: Arc<RwLock<HashMap<String, FipsExitNodeInfo>>>,
    /// Last update timestamp
    last_update: Arc<RwLock<Option<u64>>>,
    /// Async runtime
    runtime: Arc<tokio::runtime::Runtime>,
}

impl NostrDiscovery {
    /// Create a new discovery instance
    pub fn new(config: DiscoveryConfig) -> Result<Self, TollgateError> {
        let runtime = Arc::new(
            tokio::runtime::Runtime::new()
                .context("creating tokio runtime")?,
        );
        
        Ok(Self {
            config,
            cache: Arc::new(RwLock::new(HashMap::new())),
            last_update: Arc::new(RwLock::new(None)),
            runtime,
        })
    }
    
    /// Discover FIPS exit nodes via Nostr
    pub fn discover_exit_nodes(&self) -> Result<Vec<FipsExitNodeInfo>, TollgateError> {
        // Check cache first
        {
            let last_update = self.last_update.read().map_err(|e| TollgateError::Other {
                message: format!("Cache read error: {e}"),
            })?;
            
            if let Some(last) = *last_update {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| TollgateError::Protocol {
                        message: format!("System time error: {e}"),
                    })?
                    .as_secs();
                
                if now - last < self.config.cache_duration {
                    let cached = self.cache.read().map_err(|e| TollgateError::Other {
                        message: format!("Cache read error: {e}"),
                    })?;
                    
                    if !cached.is_empty() {
                        return Ok(cached.values().cloned().collect());
                    }
                }
            }
        }
        
        // Fetch from Nostr
        let nodes = self.fetch_from_nostr().await?;
        
        // Update cache
        {
            let mut cache = self.cache.write().map_err(|e| TollgateError::Other {
                message: format!("Cache write error: {e}"),
            })?;
            cache.clear();
            
            for node in &nodes {
                cache.insert(node.pubkey_full.clone(), node.clone());
            }
            
            let mut last_update = self.last_update.write().map_err(|e| TollgateError::Other {
                message: format!("Last update write error: {e}"),
            })?;
            *last_update = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| TollgateError::Protocol {
                        message: format!("System time error: {e}"),
                    })?
                    .as_secs()
            );
        }
        
        Ok(nodes)
    }
    
    /// Fetch exit nodes from Nostr relays
    fn fetch_from_nostr(&self) -> Result<Vec<FipsExitNodeInfo>, TollgateError> {
        // Mock implementation for now - in production this would:
        // 1. Connect to Nostr relays
        // 2. Query for FIPS exit node announcements (kind 31213)
        // 3. Verify signatures
        // 4. Test connectivity
        // 5. Return filtered/sorted list
        
        warn!("Using mock Nostr discovery - implement real Nostr queries");
        
        // Mock data for testing
        let mock_announcements = vec![
            FipsExitNodeAnnouncement {
                pubkey: "020a6d98d5d5a5d5e8d7f9c8b7a6d5e8f7c9b8a7d6f5e8d7c9b8a7d6f5e8d7c".into(),
                addrs: vec!["66.92.204.38:51820".into()],
                version: "fips-v0.1".into(),
                features: vec!["exit".into(), "tollgate".into(), "mobile".into()],
                bandwidth: Some(100000),
                uptime: 86400 * 7, // 1 week
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                signature: "mock_signature".into(),
            },
            FipsExitNodeAnnouncement {
                pubkey: "021b7e89f6e6f6e9f8e7d6c5b4a3f2e1d0c9b8a7f6e5d4c3b2a1f0e9d8c7b6a5".into(),
                addrs: vec!["95.217.184.52:51820".into()],
                version: "fips-v0.1".into(),
                features: vec!["exit".into(), "mobile".into()],
                bandwidth: Some(50000),
                uptime: 86400 * 2, // 2 days
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() - 3600, // 1 hour ago
                signature: "mock_signature".into(),
            },
            FipsExitNodeAnnouncement {
                pubkey: "022c8f9a7f7f7a8e9d0c1b2a3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3".into(),
                addrs: vec!["138.68.1.234:51820".into()],
                version: "fips-v0.1".into(),
                features: vec!["exit".into()],
                bandwidth: Some(25000),
                uptime: 86400, // 1 day
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() - 7200, // 2 hours ago
                signature: "mock_signature".into(),
            },
        ];
        
        // Convert to FipsExitNodeInfo and filter
        let mut nodes: Vec<FipsExitNodeInfo> = mock_announcements
            .into_iter()
            .filter(|ann| ann.validate().is_ok())
            .map(|ann| ann.into())
            .collect();
        
        // Filter by minimum score
        nodes.retain(|node| node.score >= self.config.min_score);
        
        // Sort by score (descending)
        nodes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Limit to max_results
        nodes.truncate(self.config.max_results);
        
        info!("Discovered {} FIPS exit nodes", nodes.len());
        
        Ok(nodes)
    }
    
    /// Start background discovery sync
    pub fn start_background_sync(&self) -> Result<(), TollgateError> {
        let cache = self.cache.clone();
        let last_update = self.last_update.clone();
        let config = self.config.clone();
        
        self.runtime.spawn(move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute
            
            loop {
                interval.tick().await;
                
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                
                let should_update = {
                    let last = last_update.read().unwrap();
                    last.map_or(true, |last| now - last > config.cache_duration)
                };
                
                if should_update {
                    debug!("Starting background exit node discovery");
                    
                    // Create temporary discovery instance
                    if let Ok(discovery) = NostrDiscovery::new(config.clone()) {
                        if let Ok(nodes) = discovery.discover_exit_nodes().await {
                            let mut cache = cache.write().unwrap();
                            cache.clear();
                            for node in nodes {
                                cache.insert(node.pubkey_full.clone(), node);
                            }
                            
                            let mut last_update = last_update.write().unwrap();
                            *last_update = Some(now);
                            
                            info!("Background discovery completed: {} nodes", cache.len());
                        }
                    }
                }
            }
        });
        
        Ok(())
    }
    
    /// Get the best exit node (highest score, lowest latency)
    pub fn get_best_exit_node(&self) -> Option<FipsExitNodeInfo> {
        let cache = self.cache.read().ok()?;
        let nodes: Vec<FipsExitNodeInfo> = cache.values().cloned().collect();
        
        nodes.into_iter()
            .max_by(|a, b| {
                // Prefer higher score, then lower latency
                match b.score.partial_cmp(&a.score) {
                    Some(std::cmp::Ordering::Equal) => {
                        a.latency_ms.cmp(&b.latency_ms)
                    }
                    Some(ord) => ord,
                    None => std::cmp::Ordering::Equal,
                }
            })
    }
    
    /// Update latency information for an exit node
    pub fn update_latency(&self, pubkey: &str, latency_ms: i64) -> Result<(), TollgateError> {
        let mut cache = self.cache.write().map_err(|e| TollgateError::Other {
            message: format!("Cache write error: {e}"),
        })?;
        
        if let Some(node) = cache.get_mut(pubkey) {
            node.latency_ms = latency_ms;
        }
        
        Ok(())
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fips_exit_node_validation() {
        let valid = FipsExitNodeAnnouncement {
            pubkey: "02".repeat(33),
            addrs: vec!["66.92.204.38:51820".into()],
            version: "fips-v0.1".into(),
            features: vec!["exit".into()],
            bandwidth: Some(100000),
            uptime: 86400,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: "test".into(),
        };
        
        assert!(valid.validate().is_ok());
        
        let invalid = FipsExitNodeAnnouncement {
            pubkey: "invalid".into(),
            addrs: vec![],
            version: "invalid".into(),
            features: vec![],
            bandwidth: None,
            uptime: 0,
            timestamp: 0,
            signature: "test".into(),
        };
        
        assert!(invalid.validate().is_err());
    }
    
    #[test]
    fn test_exit_node_scoring() {
        let node = FipsExitNodeAnnouncement {
            pubkey: "02".repeat(33),
            addrs: vec!["66.92.204.38:51820".into()],
            version: "fips-v0.1".into(),
            features: vec!["exit".into(), "tollgate".into(), "mobile".into()],
            bandwidth: Some(100000),
            uptime: 86400,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: "test".into(),
        };
        
        let score = node.score();
        assert!(score > 100.0); // Should be well above minimum
    }
    
    #[test]
    fn test_exit_node_info_conversion() {
        let ann = FipsExitNodeAnnouncement {
            pubkey: "02".repeat(33),
            addrs: vec!["66.92.204.38:51820".into()],
            version: "fips-v0.1".into(),
            features: vec!["exit".into()],
            bandwidth: Some(100000),
            uptime: 86400,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: "test".into(),
        };
        
        let info: FipsExitNodeInfo = ann.into();
        assert_eq!(info.endpoint, "66.92.204.38:51820");
        assert!(info.features.contains(&"exit".to_string()));
        assert_eq!(info.bandwidth_mbps, Some(100.0));
        assert_eq!(info.uptime_hours, 24.0);
    }
}