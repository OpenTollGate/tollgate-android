package org.opentollgate.android

import android.util.Log

/**
 * NativeCore - JNI bridge for TollGate Android.
 *
 * This class provides the JNI interface that allows Kotlin to call into
 * the Rust core. It handles loading the native library and declaring
 * the native functions that will be implemented in Rust.
 *
 * The native library is loaded via System.loadLibrary("tollgate_mobile"),
 * which corresponds to the libtollgate_mobile.so compiled from Rust.
 */
class NativeCore {
    
    companion object {
        
        // Load the native Rust library
        init {
            try {
                System.loadLibrary("tollgate_mobile")
                Log.d("NativeCore", "Successfully loaded tollgate_mobile native library")
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "Failed to load tollgate_mobile native library", e)
                throw NativeLibraryLoadException("Failed to load tollgate_mobile native library. Please check if the library is properly bundled.", e)
            }
        }
        
        /**
         * D3a-D3d functions - Mesh packet delivery functions
         */
        
        /**
         * Deliver a packet to the mesh network.
         * 
         * @param data The packet data to deliver
         * @return true on success, false on failure
         * @throws NativeOperationException If the JNI call fails
         */
        external fun deliverPacket(data: ByteArray): Boolean
        
        /**
         * Get the next outbound packet from the mesh.
         * 
         * @param timeoutMs Maximum time to wait for a packet in milliseconds
         * @return Packet data as ByteArray, or null if timeout or no data
         * @throws NativeOperationException If the JNI call fails
         */
        external fun nextOutboundPacket(timeoutMs: Long): ByteArray?
        
        /**
         * Start mesh TUN I/O task.
         * 
         * This function initializes app-owned TUN mode and starts the background I/O task.
         * It calls FIPS Node::enable_app_owned_tun() and stores the returned channel handles.
         * 
         * @param tunFd The TUN file descriptor from Android VpnService
         * @return true on success, false on failure
         * @throws NativeOperationException If the JNI call fails
         */
        external fun startMeshTun(tunFd: Int): Boolean
        
        /**
         * Stop mesh TUN I/O task.
         * 
         * This function signals the background task to stop and cleans up resources.
         * @throws NativeOperationException If the JNI call fails
         */
        external fun stopMeshTun()
        
        /**
         * Get FIPS peer views as JSON string.
         * Returns JSON array: [{"pubkey": "...", "endpoint": "...", "is_connected": true}]
         * @throws NativeOperationException If the JNI call fails
         */
        external fun getPeers(): String

        /**
         * Get FIPS advertisement views as JSON string.
         * Returns JSON array: [{"pubkey": "...", "price": 100, "description": "..."}]
         * @throws NativeOperationException If the JNI call fails
         */
        external fun getAdverts(): String

        /**
         * Node lifecycle (already exists from Phase 1) - JNI wrappers
         */
        
        /**
         * Create a new FIPS node instance.
         * 
         * @param dataDir Directory for node data storage
         * @return Node handle (long) for subsequent operations
         * @throws NativeOperationException If the JNI call fails
         */
        external fun newNode(dataDir: String): Long
        
        /**
         * Get the public key of a node in hex format.
         * 
         * @param handle Node handle from newNode()
         * @return Hex-encoded public key string
         * @throws NativeOperationException If the JNI call fails
         */
        external fun pubkeyHex(handle: Long): String
        
        /**
         * Detect gateway URL for the node.
         * 
         * @param handle Node handle from newNode()
         * @param gatewayUrl URL to detect against
         * @return Detection result string
         * @throws NativeOperationException If the JNI call fails
         */
        external fun detect(handle: Long, gatewayUrl: String): String
        
        /**
         * Pay through the node to a gateway.
         * 
         * @param handle Node handle from newNode()
         * @param gatewayUrl Gateway URL to pay to
         * @param token Payment token/amount
         * @return true on success, false on failure
         * @throws NativeOperationException If the JNI call fails
         */
        external fun pay(handle: Long, gatewayUrl: String, token: String): Boolean
        
        /**
         * Start consuming data through the node.
         * 
         * @param handle Node handle from newNode()
         * @param gatewayUrl Gateway URL to consume from
         * @return true on success, false on failure
         * @throws NativeOperationException If the JNI call fails
         */
        external fun startConsume(handle: Long, gatewayUrl: String): Boolean
        
        /**
         * Poll for events from the node.
         * 
         * @param handle Node handle from newNode()
         * @param timeoutMs Maximum time to wait for events in milliseconds
         * @return Event JSON string, or null if timeout
         * @throws NativeOperationException If the JNI call fails
         */
        external fun pollEvent(handle: Long, timeoutMs: Int): String?
        
        /**
         * Stop consuming data through the node.
         * 
         * @param handle Node handle from newNode()
         * @throws NativeOperationException If the JNI call fails
         */
        external fun stopConsume(handle: Long)
        
        /**
         * Safe wrapper for deliverPacket with error handling.
         */
        fun safeDeliverPacket(data: ByteArray): Result<Boolean> {
            return try {
                Result.success(deliverPacket(data))
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for deliverPacket", e)
                Result.failure(NativeLibraryLoadException("Native library not available", e))
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in deliverPacket", e)
                Result.failure(NativeOperationException("Failed to deliver packet: ${e.message}", e))
            }
        }
        
        /**
         * Safe wrapper for nextOutboundPacket with error handling.
         */
        fun safeNextOutboundPacket(timeoutMs: Long): Result<ByteArray?> {
            return try {
                Result.success(nextOutboundPacket(timeoutMs))
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for nextOutboundPacket", e)
                Result.failure(NativeLibraryLoadException("Native library not available", e))
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in nextOutboundPacket", e)
                Result.failure(NativeOperationException("Failed to get outbound packet: ${e.message}", e))
            }
        }
        
        /**
         * Safe wrapper for getPeers with error handling.
         */
        fun safeGetPeers(): Result<String> {
            return try {
                Result.success(getPeers())
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for getPeers", e)
                Result.failure(NativeLibraryLoadException("Native library not available", e))
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in getPeers", e)
                Result.failure(NativeOperationException("Failed to get peers: ${e.message}", e))
            }
        }
        
        /**
         * Safe wrapper for getAdverts with error handling.
         */
        fun safeGetAdverts(): Result<String> {
            return try {
                Result.success(getAdverts())
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for getAdverts", e)
                Result.failure(NativeLibraryLoadException("Native library not available", e))
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in getAdverts", e)
                Result.failure(NativeOperationException("Failed to get adverts: ${e.message}", e))
            }
        }
        
        /**
         * Wrapper implementations for node lifecycle functions using UniFFI interface.
         * These provide JNI-like interface while using the underlying UniFFI implementation.
         */
        
        /**
         * Create a new node instance using UniFFI interface.
         */
        fun createNode(dataDir: String): Long {
            return try {
                // Import the UniFFI TollgateMobileNode
                val nodeClass = Class.forName("uniffi.tollgate_mobile.TollgateMobileNode")
                val constructor = nodeClass.getConstructor(String::class.java)
                val node = constructor.newInstance(dataDir)
                // Use a simple hash of the object as a "handle" - in real implementation
                // this would be a proper reference
                dataDir.hashCode().toLong()
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for createNode", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in createNode", e)
                throw NativeOperationException("Failed to create node: ${e.message}", e)
            }
        }
        
        /**
         * Get the public key using UniFFI interface.
         */
        fun getNodePubkeyHex(handle: Long): String {
            return try {
                // In a real implementation, we'd store and retrieve the actual node instance
                // For now, return a placeholder that would be replaced with actual UniFFI call
                "pubkey_placeholder_${handle}"
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for getNodePubkeyHex", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in getNodePubkeyHex", e)
                throw NativeOperationException("Failed to get pubkey: ${e.message}", e)
            }
        }
        
        /**
         * Detect gateway using UniFFI interface.
         */
        fun detectGateway(handle: Long, gatewayUrl: String): String {
            return try {
                // In a real implementation, we'd call the UniFFI detect method
                // For now, return a placeholder response
                "{\"pubkey\":\"detected_pubkey\",\"unit\":\"bytes\",\"version\":1,\"price\":{\"per_second\":0,\"per_unit\":100}}"
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for detectGateway", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in detectGateway", e)
                throw NativeOperationException("Failed to detect gateway: ${e.message}", e)
            }
        }
        
        /**
         * Pay through node using UniFFI interface.
         */
        fun payThroughNode(handle: Long, gatewayUrl: String, token: String): Boolean {
            return try {
                // In a real implementation, we'd call the UniFFI pay method
                // For now, return success as placeholder
                true
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for payThroughNode", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in payThroughNode", e)
                throw NativeOperationException("Failed to pay through node: ${e.message}", e)
            }
        }
        
        /**
         * Start consuming using UniFFI interface.
         */
        fun startNodeConsume(handle: Long, gatewayUrl: String): Boolean {
            return try {
                // In a real implementation, we'd call the UniFFI startConsume method
                // For now, return success as placeholder
                true
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for startNodeConsume", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in startNodeConsume", e)
                throw NativeOperationException("Failed to start consuming: ${e.message}", e)
            }
        }
        
        /**
         * Poll for events using UniFFI interface.
         */
        fun pollNodeEvent(handle: Long, timeoutMs: Int): String? {
            return try {
                // In a real implementation, we'd call the UniFFI pollEvent method
                // For now, return null as placeholder (no events)
                null
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for pollNodeEvent", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in pollNodeEvent", e)
                throw NativeOperationException("Failed to poll event: ${e.message}", e)
            }
        }
        
        /**
         * Stop consuming using UniFFI interface.
         */
        fun stopNodeConsume(handle: Long) {
            try {
                // In a real implementation, we'd call the UniFFI stopConsume method
                // For now, it's a no-op placeholder
            } catch (e: UnsatisfiedLinkError) {
                Log.e("NativeCore", "JNI library not available for stopNodeConsume", e)
                throw NativeLibraryLoadException("Native library not available", e)
            } catch (e: RuntimeException) {
                Log.e("NativeCore", "Runtime error in stopNodeConsume", e)
                throw NativeOperationException("Failed to stop consuming: ${e.message}", e)
            }
        }
        
        /**
         * Test JNI connectivity with a safe function call.
         */
        fun isJniWorking(): Boolean {
            return try {
                // Test with a function call that should not crash
                val peers = safeGetPeers()
                peers.isSuccess
            } catch (e: Exception) {
                Log.e("NativeCore", "JNI connectivity test failed", e)
                false
            }
        }
    }
}

/**
 * Custom exception for native library loading errors.
 */
class NativeLibraryLoadException(message: String, cause: Throwable? = null) : Exception(message, cause)

/**
 * Custom exception for native operation errors.
 */
class NativeOperationException(message: String, cause: Throwable? = null) : RuntimeException(message, cause)