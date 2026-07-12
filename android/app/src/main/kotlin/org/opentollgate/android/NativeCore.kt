package org.opentollgate.android

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
object NativeCore {
    
    // Load the native Rust library
    init {
        System.loadLibrary("tollgate_mobile")
    }
    
    /**
     * Declare native functions that will be implemented in Rust.
     * 
     * These functions are the bridge between Kotlin and Rust, allowing
     * direct JNI calls without going through the UniFFI layer.
     * 
     * Currently a placeholder for future FIPS JNI integration.
     */
    
    // Example: Native function to get a greeting from Rust
    external fun nativeGreeting(): String
    
    // Example: Native function that takes a string and returns its length
    external fun stringLength(input: String): Int
    
    // Example: Native function that performs a calculation
    external fun calculateSum(a: Int, b: Int): Int
    
    /**
     * Helper function to verify JNI is working correctly.
     * This can be used for testing the JNI bridge.
     */
    fun isJniWorking(): Boolean {
        return try {
            // Test a simple native function call
            val greeting = nativeGreeting()
            greeting.isNotEmpty()
        } catch (e: Exception) {
            false
        }
    }
}

/**
 * TollGateNativeBridge - Alternative bridge for TollGate-specific JNI calls.
 *
 * This class is intended to hold TollGate-specific native functions that
 * will be implemented in Rust via JNI, complementing the UniFFI interface.
 */
object TollGateNativeBridge {
    
    /**
     * Initialize the native JNI environment.
     * This should be called early in the app lifecycle.
     */
    external fun initializeJni()
    
    /**
     * Get the native library version.
     * Returns a version string from the Rust side.
     */
    external fun getVersion(): String
    
    /**
     * Test native connectivity.
     * Returns true if the native library is responding correctly.
     */
    fun isNativeAvailable(): Boolean {
        return try {
            initializeJni()
            val version = getVersion()
            version.isNotEmpty()
        } catch (e: Exception) {
            false
        }
    }
}