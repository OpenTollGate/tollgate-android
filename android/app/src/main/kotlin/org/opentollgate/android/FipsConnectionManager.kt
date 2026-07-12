package org.opentollgate.android

import android.os.Handler
import android.os.Looper
import android.util.Log

/**
 * FipsConnectionManager - Handles FIPS reconnection retry logic.
 * 
 * FIPS has NO built-in reconnect - the app must handle it.
 * This class implements exponential backoff retry with reset on success.
 * 
 * Backoff schedule: 5s -> 10s -> 20s -> 40s -> 60s (capped at 60s)
 */
class FipsConnectionManager {

    companion object {
        private const val TAG = "FipsConnectionManager"
        
        // Backoff schedule in milliseconds: 5s -> 10s -> 20s -> 40s -> 60s (capped at 60s)
        private val backoffMs = longArrayOf(5000, 10000, 20000, 40000, 60000)
        
        // Maximum number of retry attempts before stopping
        private val MAX_ATTEMPTS = backoffMs.size
    }

    private val handler = Handler(Looper.getMainLooper())
    private var attempt = 0
    private var tunFd: Int = -1
    private var isRunning = false
    private var connectionCallback: ((Boolean) -> Unit)? = null
    
    /**
     * Start the FIPS connection with retry logic.
     * 
     * @param tunFd The TUN file descriptor from Android VpnService
     * @param callback Callback to notify connection status (true = success, false = retry)
     */
    fun startConnection(tunFd: Int, callback: (Boolean) -> Unit) {
        this.tunFd = tunFd
        this.connectionCallback = callback
        this.isRunning = true
        this.attempt = 0
        
        Log.d(TAG, "Starting FIPS connection with retry logic")
        tryConnect()
    }
    
    /**
     * Stop the retry logic and clean up resources.
     */
    fun stop() {
        Log.d(TAG, "Stopping FIPS connection manager")
        isRunning = false
        handler.removeCallbacksAndMessages(null)
        connectionCallback = null
        
        // Always stop the native mesh tunnel when stopping
        try {
            NativeCore.stopMeshTun()
        } catch (e: Exception) {
            Log.e(TAG, "Error stopping mesh tunnel", e)
        }
    }
    
    /**
     * Attempt to connect to FIPS mesh tunnel.
     * This method is called recursively with backoff until successful or max attempts reached.
     */
    private fun tryConnect() {
        if (!isRunning) {
            Log.d(TAG, "Connection manager stopped, aborting connection attempt")
            return
        }
        
        attempt++
        Log.d(TAG, "FIPS connection attempt $attempt/${MAX_ATTEMPTS}")
        
        // Run the connection attempt in a background thread to avoid blocking UI
        Thread {
            try {
                val connected = NativeCore.startMeshTun(tunFd)
                
                // Post the result back to the main thread
                handler.post {
                    if (connected) {
                        Log.d(TAG, "FIPS connection successful on attempt $attempt")
                        attempt = 0  // reset on success
                        connectionCallback?.invoke(true)
                    } else {
                        Log.w(TAG, "FIPS connection failed on attempt $attempt")
                        
                        // Check if we should retry
                        if (attempt < MAX_ATTEMPTS) {
                            val delay = backoffMs[minOf(attempt - 1, backoffMs.size - 1)]
                            Log.d(TAG, "Will retry in ${delay}ms")
                            
                            // Schedule next attempt with backoff
                            handler.postDelayed({ tryConnect() }, delay)
                        } else {
                            Log.e(TAG, "FIPS connection failed after $MAX_ATTEMPTS attempts, giving up")
                            connectionCallback?.invoke(false)
                        }
                    }
                }
            } catch (e: Exception) {
                Log.e(TAG, "Exception during FIPS connection attempt", e)
                
                // Schedule retry even on exceptions, but limit attempts
                if (attempt < MAX_ATTEMPTS) {
                    val delay = backoffMs[minOf(attempt - 1, backoffMs.size - 1)]
                    Log.d(TAG, "Will retry after exception in ${delay}ms")
                    
                    handler.postDelayed({ tryConnect() }, delay)
                } else {
                    Log.e(TAG, "FIPS connection failed after $MAX_ATTEMPTS attempts, giving up")
                    connectionCallback?.invoke(false)
                }
            }
        }.start()
    }
    
    /**
     * Get the current attempt number (for debugging/logging purposes).
     * Returns 0 when successfully connected or not running.
     */
    fun getCurrentAttempt(): Int {
        return if (attempt > 0 && isRunning) attempt else 0
    }
    
    /**
     * Check if the connection manager is actively trying to connect.
     */
    fun isConnecting(): Boolean {
        return isRunning && getCurrentAttempt() > 0
    }
}