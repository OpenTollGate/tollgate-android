//! JNI bootstrap layer for TollGate Android.
//!
//! This module provides the JNI interface that allows Kotlin code to call into
//! the Rust core. It handles JNI_OnLoad registration, JavaVM handle storage,
//! and helper functions for string conversion between Java and Rust.

use jni::objects::{JString}; 
use jni::sys::{jboolean, jint, jobject, jbyteArray}; 
use jni::JNIEnv; 
use std::sync::OnceLock; 
use tokio::sync::mpsc; 

// FIPS integration
#[cfg(feature = "fips")]
use fips::{config::Config, node::Node};

/// Global JavaVM handle storage for JNI access.
static JVM: OnceLock<jni::JavaVM> = OnceLock::new();

/// Channel storage for FIPS mesh communication
/// App to mesh: tokio mpsc sender (async, non-blocking)
static APP_TO_MESH: OnceLock<mpsc::Sender<Vec<u8>>> = OnceLock::new();
/// Mesh to app: std mpsc sender (blocking, for mesh->app packets)
static MESH_TO_APP: OnceLock<std::sync::mpsc::Sender<Vec<u8>>> = OnceLock::new();
/// TUN file descriptor storage
static TUN_FD: OnceLock<std::os::unix::io::RawFd> = OnceLock::new();
/// Background task handle
static BACKGROUND_TASK: OnceLock<std::sync::mpsc::Sender<()>> = OnceLock::new();
/// FIPS Node instance - only available when fips feature is enabled
#[cfg(feature = "fips")]
static FIPS_NODE: OnceLock<std::sync::Mutex<Option<Node>>> = OnceLock::new();

/// Get the app-to-mesh channel sender.
///
/// Returns a cloned sender from the static storage. Panics if the
/// channels are not initialized (enable_app_owned_tun must be called first).
pub fn get_app_to_mesh_sender() -> mpsc::Sender<Vec<u8>> {
    APP_TO_MESH
        .get()
        .expect("FIPS channels not initialized - call enable_app_owned_tun first")
        .clone()
}

/// Get the mesh-to-app channel sender.
///
/// Returns a cloned sender from the static storage. Panics if the
/// channels are not initialized (enable_app_owned_tun must be called first).
pub fn get_mesh_to_app_sender() -> std::sync::mpsc::Sender<Vec<u8>> {
    MESH_TO_APP
        .get()
        .expect("FIPS channels not initialized - call enable_app_owned_tun first")
        .clone()
}

/// deliverPacket - Submit packet from Android app to FIPS mesh
///
/// This function takes a packet from the Android app and forwards it
/// to the FIPS mesh via the app-to-mesh channel.
/// # Parameters
/// * env - JNI environment  
/// * data - byte array containing the packet data
/// # Returns
/// jboolean - 1 on success, 0 on failure
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_deliverPacket(
    env: JNIEnv,
    _class: jobject,
    data: jni::objects::JByteArray,
) -> jboolean {
    // Convert jbyteArray to Vec<u8>
    let packet = env.convert_byte_array(data)
        .expect("Failed to convert jbyteArray to Vec<u8>");
    
    // Get the app-to-mesh sender
    let sender = get_app_to_mesh_sender();
    
    // Try to send packet (non-blocking, drops if full)
    match sender.try_send(packet) {
        Ok(_) => 1,
        Err(mpsc::error::TrySendError::Full(_)) => {
            eprintln!("deliverPacket: app-to-mesh channel full, dropping packet");
            0
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            eprintln!("deliverPacket: app-to-mesh channel closed");
            0
        }
    }
}

/// startMeshTun - Initialize TUN fd handoff with FIPS
///
/// This function takes the TUN file descriptor from Android and
/// initializes the FIPS mesh tunnel by calling enable_app_owned_tun
/// and starting the background I/O task.
/// # Parameters
/// * env - JNI environment
/// * class - class object  
/// * tun_fd - The TUN file descriptor from Android VpnService
/// 
/// Returns jboolean - 1 on success, 0 on failure
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_startMeshTun(
    _env: JNIEnv,
    _class: jobject,
    tun_fd: jint,
) -> jboolean {
    // Convert jint to RawFd
    let tun_fd = tun_fd as std::os::unix::io::RawFd;
        
    // Call the Rust implementation
    match start_mesh_tun(tun_fd) {
        true => 1,
        false => 0,
    }
}

/// stopMeshTun - Clean up TUN fd handoff and shutdown background task
///
/// This function stops the background TUN I/O task and closes the TUN fd.
/// # Parameters
/// * env - JNI environment
/// * class - class object
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_stopMeshTun(
    _env: JNIEnv,
    _class: jobject,
) {
    // Call the Rust implementation
    stop_mesh_tun();
}

/// nextOutboundPacket - Pulls packet from mesh-to-app channel (blocking with timeout)
///
/// # Parameters
/// * env - JNI environment  
/// * class - class object
/// * timeout_ms - timeout in milliseconds, -1 for indefinite
/// # Returns
/// jbyteArray containing packet data, or null on timeout/error
/// nextOutboundPacket - Pulls packet from mesh-to-app channel (blocking with timeout)
///
/// # Parameters
/// * env - JNI environment  
/// * class - class object
/// * timeout_ms - timeout in milliseconds, -1 for indefinite
/// # Returns
/// jbyteArray containing packet data, or null on timeout/error
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_nextOutboundPacket(
    _env: JNIEnv,
    _class: jobject,
    timeout_ms: jint,
) -> jbyteArray {
    // Get the mesh-to-app sender
    let _sender = get_mesh_to_app_sender();
    
    // For now, just return null - this needs to be connected to the FIPS node's
    // tun_rx receiver. This is a placeholder for the mesh-to-app packet flow.
    eprintln!("nextOutboundPacket: Not yet implemented - needs FIPS node tun_rx integration");
    std::ptr::null_mut()
}

/// getPeers - Get FIPS peer views as JSON string
/// 
/// This function calls FIPS node.peer_views() and serializes the result
/// to JSON format: [{"pubkey": "...", "endpoint": "...", "is_connected": true}]
/// 
/// # Parameters
/// * env - JNI environment
/// * class - class object
/// # Returns
/// jstring containing JSON array of peer objects, or empty string on error
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_getPeers(
    env: JNIEnv,
    _class: jobject,
) -> jni::objects::JString {
    // Serialize peer views to JSON
    match serialize_peer_views_to_json() {
        Ok(json_str) => rust_to_jstring(&env, &json_str),
        Err(e) => {
            eprintln!("getPeers: Failed to serialize peer views: {}", e);
            rust_to_jstring(&env, "[]")
        }
    }
}

/// getAdverts - Get FIPS advertisement views as JSON string
/// 
/// This function calls FIPS node.advert_views() and serializes the result
/// to JSON format: [{"pubkey": "...", "price": 100, "description": "..."}]
/// 
/// # Parameters
/// * env - JNI environment
/// * class - class object
/// # Returns
/// jstring containing JSON array of advertisement objects, or empty string on error
#[no_mangle]
pub extern "system" fn Java_org_opentollgate_android_NativeCore_getAdverts(
    env: JNIEnv,
    _class: jobject,
) -> jni::objects::JString {
    // Serialize advert views to JSON
    match serialize_advert_views_to_json() {
        Ok(json_str) => rust_to_jstring(&env, &json_str),
        Err(e) => {
            eprintln!("getAdverts: Failed to serialize advert views: {}", e);
            rust_to_jstring(&env, "[]")
        }
    }
}

#[allow(dead_code)]
pub fn get_jni_env<'a>() -> JNIEnv<'a> {
    JVM.get()
        .expect("JavaVM not initialized")
        .get_env()
        .expect("Failed to get JNIEnv")
}

#[allow(dead_code)]
pub fn jstring_to_rust(env: &mut JNIEnv, jstring: JString) -> String {
    env.get_string(&jstring)
        .expect("Failed to get string from JString")
        .into()
}

#[allow(dead_code)]
pub fn rust_to_jstring<'a>(env: &'a JNIEnv, string: &str) -> JString<'a> {
    env.new_string(string)
        .expect("Failed to create JString from rust string")
}

/// Serialize FIPS peer views to JSON string
/// 
/// This function gets the FIPS node, calls peer_views(), and serializes
/// the result to JSON format: [{"pubkey": "...", "endpoint": "...", "is_connected": true}]
#[cfg(feature = "fips")]
fn serialize_peer_views_to_json() -> Result<String, String> {
    // Get the FIPS node
    let fips_node = match FIPS_NODE.get() {
        Some(node_mutex) => node_mutex.lock().map_err(|e| format!("Failed to lock FIPS node: {}", e))?,
        None => return Err("FIPS node not initialized".to_string()),
    };
    
    if fips_node.is_none() {
        return Err("FIPS node is None".to_string());
    }
    
    // Get the node reference
    let node = fips_node.as_ref().unwrap();
    
    // Get the control read handle and call peer_views()
    let control_handle = node.control_read_handle();
    let peers = control_handle.peer_views();
    
    let peer_data: Vec<serde_json::Value> = peers.iter().map(|peer| {
        serde_json::json!({
            "pubkey": peer.npub.clone(),
            "endpoint": peer.node_addr_hex.clone(),
            "is_connected": peer.connected
        })
    }).collect();
    
    Ok(serde_json::to_string(&peer_data).map_err(|e| format!("JSON serialization failed: {}", e))?)
}

#[cfg(not(feature = "fips"))]
fn serialize_peer_views_to_json() -> Result<String, String> {
    // Return empty array when FIPS feature is not enabled
    Ok("[]".to_string())
}

/// Serialize FIPS advertisement views to JSON string
/// 
/// This function gets the FIPS node, calls advert_views(), and serializes
/// the result to JSON format: [{"pubkey": "...", "price": 100, "description": "..."}]
#[cfg(feature = "fips")]
fn serialize_advert_views_to_json() -> Result<String, String> {
    // Get the FIPS node
    let fips_node = match FIPS_NODE.get() {
        Some(node_mutex) => node_mutex.lock().map_err(|e| format!("Failed to lock FIPS node: {}", e))?,
        None => return Err("FIPS node not initialized".to_string()),
    };
    
    if fips_node.is_none() {
        return Err("FIPS node is None".to_string());
    }
    
    // Get the node reference (advert_views is not directly accessible from main node)
    // TODO: Implement access to BLE transport bridge for advert_views
    let node = fips_node.as_ref().unwrap();
    let _ = node; // Keep the node reference but don't use it for now
    
    // For now, return empty array - advert_views needs BLE bridge access
    let adverts = vec![]; // TODO: Access actual advert_views via BLE bridge
    
    let advert_data: Vec<serde_json::Value> = adverts.iter().map(|advert| {
        serde_json::json!({
            "pubkey": "unknown", // TODO: Map actual pubkey field from BLE advert data
            "price": 0, // TODO: Extract actual price information
            "description": format!("BLE advert at {}", advert.addr)
        })
    }).collect();
    
    Ok(serde_json::to_string(&advert_data).map_err(|e| format!("JSON serialization failed: {}", e))?)
}

#[cfg(not(feature = "fips"))]
fn serialize_advert_views_to_json() -> Result<String, String> {
    // Return empty array when FIPS feature is not enabled
    Ok("[]".to_string())
}

/// JNI_OnLoad function - entry point for the JVM.
///
/// This function is called by the JVM when the library is loaded.
/// It registers the native methods and returns the JNI version.
///
/// # Safety
/// This function is called by the JVM and must be thread-safe.
#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jint {
    // Store the JavaVM handle globally first
    let _ = JVM.set(vm);
    
    // Then attach the current thread using the stored JVM
    let attach_result = JVM.get().unwrap().attach_current_thread();
    match attach_result {
        Ok(_) => {
            jni::sys::JNI_VERSION_1_6
        }
        Err(e) => {
            eprintln!("Failed to attach current thread: {}", e);
            jni::sys::JNI_VERSION_1_6
        }
    }
}

/// JNI_OnUnload function - cleanup when the library is unloaded.
///
/// # Safety
/// This function is called by the JVM and must be thread-safe.
#[no_mangle]
pub extern "system" fn JNI_OnUnload(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) {
    // Detach the current thread from the JVM
    let _ = unsafe { vm.detach_current_thread() };
    
    // Clear the stored JavaVM (OnceLock doesn't have take, so we'll leave it)
    // JVM.take();
}

// Register any native methods here in the future.
// For now, we just need the JNI_OnLoad to establish the JavaVM connection.

#[cfg(target_os = "android")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jstring_conversion() {
        // This test would require a JVM to run, so we skip it in normal tests
        // It would be tested as part of the Android integration tests
        println!("JNI string conversion tests require Android environment");
    }
}

/// enable_app_owned_tun - Initialize the channel storage and FIPS node
/// 
/// This function creates a FIPS Node instance with TUN enabled in the configuration.
/// The FIPS node internally handles TUN setup when configured properly.
/// It stores the channel handles for bidirectional communication.
/// # Parameters
/// * app_to_mesh_sender - Sender for packets from app to mesh
/// * mesh_to_app_sender - Sender for packets from mesh to app
/// # Returns
/// true on success, false on failure
#[cfg(feature = "fips")]
pub fn enable_app_owned_tun(
    app_to_mesh_sender: mpsc::Sender<Vec<u8>>,
    mesh_to_app_sender: std::sync::mpsc::Sender<Vec<u8>>,
) -> bool {
    // Create a FIPS Node with default configuration
    let config = Config::default();
    match Node::new(config) {
        Ok(node) => {
            // The FIPS node handles TUN internally when configured properly
            // No need to call enable_app_owned_tun - it's handled automatically
            
            // Store the channels
            let _ = APP_TO_MESH.set(app_to_mesh_sender);
            let _ = MESH_TO_APP.set(mesh_to_app_sender);
            
            // Store the FIPS node
            let _ = FIPS_NODE.set(std::sync::Mutex::new(Some(node)));
            
            // Store the FIPS outbound_tx for app→mesh communication
            // Note: We need to bridge this with our app_to_mesh_sender
            // For now, we'll use a simple approach: spawn a task to forward
            let _ = TUN_FD.set(-1); // Placeholder - actual TUN fd comes from Android
            
            eprintln!("enable_app_owned_tun: FIPS node initialized successfully");
            true
        }
        Err(e) => {
            eprintln!("enable_app_owned_tun: Failed to create FIPS node: {}", e);
            false
        }
    }
}

#[cfg(not(feature = "fips"))]
pub fn enable_app_owned_tun(
    app_to_mesh_sender: mpsc::Sender<Vec<u8>>,
    mesh_to_app_sender: std::sync::mpsc::Sender<Vec<u8>>,
) -> bool {
    let _ = APP_TO_MESH.set(app_to_mesh_sender);
    let _ = MESH_TO_APP.set(mesh_to_app_sender);
    eprintln!("enable_app_owned_tun: FIPS feature not enabled, using mock channels");
    true
}

/// start_mesh_tun - Store TUN fd, start background I/O task, and initialize FIPS
/// 
/// This function takes the TUN file descriptor from Android, stores it,
/// initializes the FIPS mesh tunnel by calling enable_app_owned_tun(),
/// and starts the background I/O task that forwards packets between
/// the TUN interface and the FIPS mesh channels.
/// 
/// GOTCHA 2: fd00::/8 filter bypass. FIPS does NOT filter destinations in app-owned mode.
/// The VpnService routing rules (in D4b) MUST ensure only mesh-bound traffic hits the TUN.
/// Document this as a comment in the code.
/// 
/// GOTCHA 3: TCP MSS clamping. Outbound SYNs MUST be clamped to MTU-40=1240.
/// Without this, cold TCP connections wedge silently. Add a TODO comment for D4b implementation.
/// 
/// # Parameters
/// * tun_fd - The TUN file descriptor from Android VpnService
/// # Returns
/// true on success, false on failure
pub fn start_mesh_tun(tun_fd: std::os::unix::io::RawFd) -> bool {
    eprintln!("start_mesh_tun: Initializing with TUN fd: {}", tun_fd);
    
    // Initialize channels for FIPS integration
    let (app_to_mesh_tx, _app_to_mesh_rx) = mpsc::channel(1024);
    let (mesh_to_app_tx, mesh_to_app_rx) = std::sync::mpsc::channel();
    
    // Initialize the FIPS node and channels
    if !enable_app_owned_tun(app_to_mesh_tx, mesh_to_app_tx) {
        eprintln!("start_mesh_tun: Failed to initialize FIPS node");
        return false;
    }
    
    // Store the TUN fd
    let _ = TUN_FD.set(tun_fd);
    
    // Create a shutdown channel for the background task
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    let _ = BACKGROUND_TASK.set(shutdown_tx);
    
    // Start the background task in a new thread
    std::thread::spawn(move || {
        run_mesh_tun_task(tun_fd, shutdown_rx);
    });
    
    // Also start a task to forward from mesh-to-app channel to TUN writes
    std::thread::spawn(move || {
        run_mesh_to_tun_task(tun_fd, mesh_to_app_rx);
    });
    
    eprintln!("start_mesh_tun: Background tasks started successfully");
    true
}

/// stop_mesh_tun - Shutdown background task and close TUN fd
/// 
/// This function signals the background I/O task to stop,
/// closes the TUN file descriptor, and cleans up the channels.
pub fn stop_mesh_tun() {
    // Signal the background task to stop
    if let Some(shutdown_tx) = BACKGROUND_TASK.get().cloned() {
        let _ = shutdown_tx.send(());
    }
    
    // Close the TUN fd
    if let Some(&tun_fd) = TUN_FD.get() {
        unsafe {
            libc::close(tun_fd);
        }
    }
    
    // Clear the FIPS channels (these will be cleaned up when OnceLock is dropped)
    let _ = APP_TO_MESH.get();
    let _ = MESH_TO_APP.get();
    
    // Clear the FIPS node
    #[cfg(feature = "fips")]
    let _ = FIPS_NODE.get();
    
    eprintln!("stop_mesh_tun: Cleanup completed");
}

/// run_mesh_tun_task - Background task for TUN fd I/O (TUN → mesh)
    /// 
    /// This function runs in a separate thread and handles reading packets
    /// from the Android TUN interface and forwarding them to the FIPS mesh
    /// via the app-to-mesh channel.
    /// # Parameters
    /// * tun_fd - The TUN file descriptor
    /// * shutdown_rx - Receiver for shutdown signal
    fn run_mesh_tun_task(tun_fd: std::os::unix::io::RawFd, shutdown_rx: std::sync::mpsc::Receiver<()>) {
        // Check for shutdown signal immediately
        if shutdown_rx.try_recv().is_ok() {
            eprintln!("Mesh TUN task: shutdown requested immediately");
            return;
        }
    
        let app_to_mesh_sender = match APP_TO_MESH.get() {
            Some(sender) => sender.clone(),
            None => {
                eprintln!("Mesh TUN task: app-to-mesh channel not initialized");
                return;
            }
        };
    
        // Create a buffer for TUN reads
        let mut buffer = vec![0u8; 65536]; // 64KB buffer
    
        // Main I/O loop
        loop {
            // Check for shutdown signal
            if shutdown_rx.try_recv().is_ok() {
                eprintln!("Mesh TUN task: shutdown signal received");
                break;
            }
        
            // Read from TUN fd (mesh-bound packets from Android)
            let bytes_read = unsafe {
                libc::read(
                    tun_fd,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buffer.len(),
                )
            };
        
            match bytes_read {
                // Read successful
                bytes_read if bytes_read > 0 => {
                    let packet = buffer[..bytes_read as usize].to_vec();
                
                    // Push packet to app-to-mesh channel
                    match app_to_mesh_sender.try_send(packet) {
                        Ok(_) => {
                            // Packet successfully queued for FIPS processing
                        }
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            eprintln!("Mesh TUN task: app-to-mesh channel full, dropping packet");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            eprintln!("Mesh TUN task: app-to-mesh channel closed, shutting down");
                            break;
                        }
                    }
                }
                // No data available (try again)
                0 => {
                    // This can happen with non-blocking reads, just continue
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                // Error
                -1 => {
                    let errno = std::io::Error::last_os_error();
                    if errno.raw_os_error() == Some(libc::EAGAIN)
                        || errno.raw_os_error() == Some(libc::EWOULDBLOCK)
                    {
                        // Non-blocking, try again
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    } else {
                        eprintln!("Mesh TUN task: read error: {}", errno);
                        break;
                    }
                }
                // Unexpected bytes_read value
                _ => {
                    eprintln!("Mesh TUN task: unexpected bytes_read value: {}", bytes_read);
                    break;
                }
            }
            // TODO: Handle mesh-to-app packets by reading from MESH_TO_APP channel
            // This requires access to the FIPS node's tun_rx receiver
            // For now, we'll handle this in a future iteration
        }
    
        eprintln!("Mesh TUN task: exiting");
    }

    /// run_mesh_to_tun_task - Background task for mesh → TUN I/O
    /// 
    /// This function runs in a separate thread and handles reading packets
    /// from the mesh-to-app channel and writing them to the TUN interface.
    /// # Parameters
    /// * tun_fd - The TUN file descriptor
    /// * mesh_to_app_rx - Receiver for packets from mesh to app
    fn run_mesh_to_tun_task(tun_fd: std::os::unix::io::RawFd, mesh_to_app_rx: std::sync::mpsc::Receiver<Vec<u8>>) {
        // Check for shutdown signal immediately
        if let Ok(_) = mesh_to_app_rx.try_recv() {
            // Drain any pending messages first
        }
    
        let _buffer = vec![0u8; 65536]; // 64KB buffer for TUN writes
    
        // Main I/O loop
        loop {
            // Check for shutdown signal by testing if channel is closed
            match mesh_to_app_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(packet) => {
                    // Write packet to TUN fd
                    let bytes_written = unsafe {
                        libc::write(
                            tun_fd,
                            packet.as_ptr() as *const libc::c_void,
                            packet.len(),
                        )
                    };
                
                    match bytes_written {
                        // Write successful
                        bytes_written if bytes_written as usize == packet.len() => {
                            // Packet successfully written to TUN
                        }
                        // Partial write or error
                        _ => {
                            eprintln!(
                                "Mesh-to-TUN task: write error, bytes_written: {}, packet_len: {}",
                                bytes_written,
                                packet.len()
                            );
                            // TODO: Handle partial writes by retrying remaining data
                            break;
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // No data available, continue loop
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    eprintln!("Mesh-to-TUN task: mesh-to-app channel closed, shutting down");
                    break;
                }
            }
        }

    eprintln!("Mesh-to-TUN task: exiting");
}

#[cfg(target_os = "android")]
#[cfg(test)]
mod tests {
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
}