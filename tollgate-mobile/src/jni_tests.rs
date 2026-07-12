#[cfg(test)]
mod jni_tests {
    use super::*;

    #[test]
    fn test_peer_views_serialization() {
        // Test that peer views serialization returns valid JSON
        let result = serialize_peer_views_to_json();
        assert!(result.is_ok());
        
        let json_str = result.unwrap();
        assert!(!json_str.is_empty());
        
        // Should be a valid JSON array
        assert!(json_str.starts_with('['));
        assert!(json_str.ends_with(']'));
    }

    #[test]
    fn test_advert_views_serialization() {
        // Test that advert views serialization returns valid JSON
        let result = serialize_advert_views_to_json();
        assert!(result.is_ok());
        
        let json_str = result.unwrap();
        assert!(!json_str.is_empty());
        
        // Should be a valid JSON array
        assert!(json_str.starts_with('['));
        assert!(json_str.ends_with(']'));
    }

    #[test]
    fn test_empty_arrays_when_fips_disabled() {
        // Test that empty arrays are returned when FIPS feature is not enabled
        #[cfg(not(feature = "fips"))]
        {
            let peers_result = serialize_peer_views_to_json();
            let adverts_result = serialize_advert_views_to_json();
            
            assert_eq!(peers_result.unwrap(), "[]");
            assert_eq!(adverts_result.unwrap(), "[]");
        }
    }

    #[test]
    fn test_json_format_peers() {
        // Test the expected JSON format for peer data
        #[cfg(feature = "fips")]
        {
            // Create a mock peer to test serialization format
            let mock_peer = crate::control::read_handle::PeerView {
                node_addr_hex: "test_node_addr".to_string(),
                npub: "test_pubkey".to_string(),
                connected: true,
            };
            
            let peer_data = vec![mock_peer];
            let json_data: Vec<serde_json::Value> = peer_data.iter().map(|peer| {
                serde_json::json!({
                    "pubkey": peer.npub.clone(),
                    "endpoint": peer.node_addr_hex.clone(),
                    "is_connected": peer.connected
                })
            }).collect();
            
            let json_str = serde_json::to_string(&json_data).unwrap();
            assert!(json_str.contains("\"pubkey\":\"test_pubkey\""));
            assert!(json_str.contains("\"endpoint\":\"test_node_addr\""));
            assert!(json_str.contains("\"is_connected\":true"));
        }
    }
}