package org.opentollgate.android

import android.util.Log
import java.net.*
import java.util.concurrent.atomic.AtomicBoolean

/**
 * UdpTransport - Custom UDP transport for FIPS gateway.
 *
 * This transport mirrors AndroidBleBridge: Kotlin owns a DatagramSocket to VPS1
 * (66.92.204.38 port 2121) and exchanges bytes via the channel bridge.
 *
 * The transport runs on dedicated threads and provides:
 * - Incoming UDP packets -> deliver_recv channel (push to mesh)
 * - Outgoing mesh packets -> next_send channel (pull and transmit)
 *
 * This replaces the BLE byte-bridge for WiFi/Internet-connected phones.
 */
class UdpTransport(
    private val gatewayHost: String = "66.92.204.38",
    private val gatewayPort: Int = 2121
) {
    
    companion object {
        private const val TAG = "UdpTransport"
        private const val SOCKET_TIMEOUT_MS = 100  // non-blocking-ish read
        private const val SEND_POLL_TIMEOUT_MS = 100  // timeout for polling outbound packets
        private const val MAX_PACKET_SIZE = 1500  // MTU for UDP packets
    }
    
    private var socket: DatagramSocket? = null
    private val running = AtomicBoolean(false)
    private var receiveThread: Thread? = null
    private var sendThread: Thread? = null
    
    /**
     * Start the UDP transport.
     * 
     * Creates and connects a DatagramSocket to the FIPS gateway,
     * then starts dedicated receive and send threads.
     */
    fun start() {
        if (running.get()) {
            Log.w(TAG, "Transport already running")
            return
        }
        
        Log.d(TAG, "Starting UDP transport to $gatewayHost:$gatewayPort")
        
        try {
            // Create and configure the socket
            socket = DatagramSocket().apply {
                connect(InetAddress.getByName(gatewayHost), gatewayPort)
                soTimeout = SOCKET_TIMEOUT_MS
                Log.d(TAG, "Socket connected to $gatewayHost:$gatewayPort")
            }
            
            running.set(true)
            
            // Start receive thread: socket -> mesh channel
            receiveThread = Thread(this::receiveLoop, "UdpTransport-Receive").apply {
                start()
                Log.d(TAG, "Receive thread started")
            }
            
            // Start send thread: mesh channel -> socket
            sendThread = Thread(this::sendLoop, "UdpTransport-Send").apply {
                start()
                Log.d(TAG, "Send thread started")
            }
            
            Log.d(TAG, "UDP transport started successfully")
            
        } catch (e: Exception) {
            Log.e(TAG, "Failed to start UDP transport", e)
            stop() // Clean up on failure
            throw UdpTransportException("Failed to start UDP transport: ${e.message}", e)
        }
    }
    
    /**
     * Stop the UDP transport.
     * 
     * Signals both threads to stop and closes the socket.
     */
    fun stop() {
        Log.d(TAG, "Stopping UDP transport")
        
        running.set(false)
        
        // Wait for threads to finish with timeout
        receiveThread?.join(1000)
        sendThread?.join(1000)
        
        // Close socket
        socket?.let { socket ->
            if (!socket.isClosed) {
                try {
                    socket.close()
                    Log.d(TAG, "Socket closed")
                } catch (e: Exception) {
                    Log.e(TAG, "Error closing socket", e)
                }
            }
        }
        
        receiveThread = null
        sendThread = null
        socket = null
        
        Log.d(TAG, "UDP transport stopped")
    }
    
    /**
     * Receive loop: socket -> mesh channel.
     * 
     * Continuously reads from the DatagramSocket and delivers packets
     * to the mesh via NativeCore.deliverPacket().
     */
    private fun receiveLoop() {
        val buffer = ByteArray(MAX_PACKET_SIZE)
        
        Log.d(TAG, "Receive loop started")
        
        while (running.get()) {
            try {
                val packet = DatagramPacket(buffer, buffer.size)
                socket?.receive(packet)
                
                // Deliver packet to mesh (copy data to avoid buffer reuse)
                val packetData = packet.data.copyOf(packet.length)
                val delivered = NativeCore.deliverPacket(packetData)
                
                if (delivered) {
                    Log.v(TAG, "Delivered ${packetData.size} bytes to mesh")
                } else {
                    Log.w(TAG, "Failed to deliver ${packetData.size} bytes to mesh")
                }
                
            } catch (e: SocketTimeoutException) {
                // Normal - continue loop for non-blocking-ish behavior
                continue
            } catch (e: Exception) {
                Log.w(TAG, "Error in receive loop", e)
                if (running.get()) {
                    // Log but continue running unless transport is stopping
                    Thread.sleep(100) // Small delay to prevent busy loop
                }
            }
        }
        
        Log.d(TAG, "Receive loop ended")
    }
    
    /**
     * Send loop: mesh channel -> socket.
     * 
     * Continuously polls NativeCore.nextOutboundPacket() for outgoing
     * packets and sends them via the DatagramSocket.
     */
    private fun sendLoop() {
        Log.d(TAG, "Send loop started")
        
        while (running.get()) {
            try {
                // Get next outbound packet from mesh
                val data = NativeCore.nextOutboundPacket(SEND_POLL_TIMEOUT_MS.toLong())
                
                if (data != null) {
                    // Send packet via socket
                    val packet = DatagramPacket(data, data.size)
                    socket?.send(packet)
                    
                    Log.v(TAG, "Sent ${data.size} bytes to gateway")
                } else {
                    // No data available, small delay to prevent busy loop
                    Thread.sleep(10)
                }
                
            } catch (e: Exception) {
                Log.w(TAG, "Error in send loop", e)
                if (running.get()) {
                    // Small delay to prevent busy loop on persistent errors
                    Thread.sleep(100)
                }
            }
        }
        
        Log.d(TAG, "Send loop ended")
    }
    
    /**
     * Check if the transport is currently running.
     */
    fun isRunning(): Boolean {
        return running.get() && (receiveThread?.isAlive == true || sendThread?.isAlive == true)
    }
}

/**
 * Custom exception for UDP transport errors.
 */
class UdpTransportException(message: String, cause: Throwable? = null) : Exception(message, cause)