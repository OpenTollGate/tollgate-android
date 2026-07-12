package org.opentollgate.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.ParcelFileDescriptor
import androidx.core.app.NotificationCompat
import android.net.VpnService as AndroidVpnService

/**
 * TollGateVpnService - Android VpnService that creates the TUN fd, sets up routing rules,
 * and hands the fd to FIPS via JNI.
 *
 * This is where the 3 gotchas are enforced. The VpnService controls which traffic goes
 * through the FIPS mesh tunnel.
 */
class TollGateVpnService : AndroidVpnService() {

    companion object {
        private const val CHANNEL_ID = "TollGateVpnService"
        private const val NOTIFICATION_ID = 1
        
        // Singleton FipsConnectionManager instance for the service lifecycle
        private var connectionManager: FipsConnectionManager? = null
        
        /**
         * Start the VpnService to establish the FIPS mesh tunnel.
         */
        fun start(context: android.content.Context) {
            val intent = Intent(context, TollGateVpnService::class.java).apply {
                action = "START_VPN"
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }
        
        /**
         * Stop the VpnService.
         */
        fun stop(context: android.content.Context) {
            val intent = Intent(context, TollGateVpnService::class.java).apply {
                action = "STOP_VPN"
            }
            context.stopService(intent)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            "START_VPN" -> establishVpnConnection()
            "STOP_VPN" -> stopVpnConnection()
            else -> establishVpnConnection() // Default action
        }
        
        return START_STICKY
    }

    private fun establishVpnConnection() {
        try {
            // Create VPN builder
            val builder = Builder()
            
            // Set VPN session name
            builder.setSession("TollGate FIPS Mesh")
            
            // GOTCHA 3: TCP MSS clamping
            // Outbound SYNs MUST be clamped to MTU-40=1240. Without this, cold TCP 
            // connections wedge silently. This is the #1 cause of 'connection established 
            // but no data flows' bugs.
            builder.setMtu(1280)  // GOTCHA 3: MSS = MTU - 40 = 1240
            
            // Add address for the FIPS mesh interface
            builder.addAddress("10.66.0.2", 16)  // FIPS mesh address /16
            
            // GOTCHA 2: fd00::/8 filter bypass
            // FIPS does NOT filter destinations in app-owned mode. The VpnService 
            // routing rules MUST ensure only mesh-bound traffic hits the TUN.
            // - AddRoute() for ONLY the FIPS mesh subnet (e.g. 10.66.0.0/16)
            // - Do NOT add 0.0.0.0/0 or ::/0 - that would tunnel ALL traffic
            // - AddAllowedApplication() for the TollGate app itself (to prevent routing loops)
            builder.addRoute("10.66.0.0", 16)    // GOTCHA 2: ONLY mesh subnet
            builder.addAllowedApplication(packageName)  // prevent routing loop
            
            // Set non-blocking mode for efficient packet handling
            builder.setBlocking(false)
            
            // Establish the VPN interface and get the TUN file descriptor
            val tunInterface: ParcelFileDescriptor? = builder.establish()
            if (tunInterface == null) {
                android.util.Log.e("TollGateVpnService", "Failed to establish VPN interface")
                return
            }
            
            // Start the foreground service notification
            startForeground(NOTIFICATION_ID, createNotification())
            
            // Extract the file descriptor and prepare for FIPS connection with retry logic
            val tunFd = tunInterface.detachFd()
            android.util.Log.d("TollGateVpnService", "VPN established with TUN fd: $tunFd")
            
            // Initialize or reuse the FipsConnectionManager
            if (connectionManager == null) {
                connectionManager = FipsConnectionManager()
            }
            val manager = connectionManager
            
            // Start FIPS connection with retry logic
            manager?.startConnection(tunFd) { success ->
                if (success) {
                    android.util.Log.d("TollGateVpnService", "FIPS mesh tunnel started successfully with retry logic")
                } else {
                    android.util.Log.e("TollGateVpnService", "FIPS mesh tunnel failed after all retry attempts")
                    tunInterface.close()
                    // Consider stopping the service if connection failed completely
                    // stopSelf()
                }
            }
            
        } catch (e: Exception) {
            android.util.Log.e("TollGateVpnService", "Error establishing VPN connection", e)
        }
    }

    private fun stopVpnConnection() {
        try {
            // Stop the FIPS connection manager (this will handle native cleanup)
            connectionManager?.stop()
            connectionManager = null
            
            android.util.Log.d("TollGateVpnService", "FIPS connection manager stopped")
            
            // Stop the service
            stopSelf()
            
        } catch (e: Exception) {
            android.util.Log.e("TollGateVpnService", "Error stopping VPN connection", e)
        }
    }

    private fun createNotification(): Notification {
        val notificationManager = getSystemService(NotificationManager::class.java)
        
        // Create notification channel for Android 8.0+
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "TollGate VPN Service",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "TollGate FIPS mesh tunnel is active"
                setShowBadge(false)
                setSound(null, null)
            }
            notificationManager.createNotificationChannel(channel)
        }
        
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("TollGate VPN")
            .setContentText("FIPS mesh tunnel is active")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setOngoing(true)
            .build()
    }

    override fun onDestroy() {
        try {
            // Stop the FIPS connection manager (this will handle native cleanup)
            connectionManager?.stop()
            connectionManager = null
            
            android.util.Log.d("TollGateVpnService", "onDestroy: FIPS connection manager stopped")
        } catch (e: Exception) {
            android.util.Log.e("TollGateVpnService", "Error in onDestroy", e)
        }
        super.onDestroy()
    }

    override fun onRevoke() {
        try {
            // VPN service was revoked by system or user - immediately stop the FIPS connection manager
            connectionManager?.stop()
            connectionManager = null
            
            android.util.Log.d("TollGateVpnService", "onRevoke: FIPS connection manager stopped")
            
            // Stop the service
            stopSelf()
            
        } catch (e: Exception) {
            android.util.Log.e("TollGateVpnService", "Error in onRevoke", e)
        }
        super.onRevoke()
    }
}