package org.opentollgate.android.util

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class FormatTest {
    @Test
    fun formatBytes_zero() {
        assertEquals("0 B", formatBytes(0))
    }

    @Test
    fun formatBytes_512() {
        assertEquals("512 B", formatBytes(512))
    }

    @Test
    fun formatBytes_1000() {
        assertEquals("1.0 KB", formatBytes(1000))
    }

    @Test
    fun formatBytes_1MB() {
        assertEquals("1.0 MB", formatBytes(1048576))
    }

    @Test
    fun formatDurationMillis_zero() {
        assertEquals("0s", formatDurationMillis(0))
    }

    @Test
    fun formatDurationMillis_5s() {
        assertEquals("5s", formatDurationMillis(5000))
    }

    @Test
    fun formatDurationMillis_1m05s() {
        assertEquals("1m 05s", formatDurationMillis(65000))
    }

    @Test
    fun formatDurationMillis_1h1m01s() {
        assertEquals("1h 1m 01s", formatDurationMillis(3661000))
    }

    @Test
    fun shortPubkey_truncates() {
        val hex = "a".repeat(66)
        val result = shortPubkey(hex, 16)
        assertEquals(17, result.length, "should be 16 chars + ellipsis")
        assertTrue(result.endsWith("\u2026"))
    }

    @Test
    fun shortPubkey_short_unchanged() {
        val short = "abcdef1234"
        assertEquals(short, shortPubkey(short, 16))
    }

    @Test
    fun formatScaledSats_null() {
        assertNull(formatScaledSats(null))
    }

    @Test
    fun formatScaledSats_zero() {
        assertEquals("free", formatScaledSats(0))
    }

    @Test
    fun formatScaledSats_1000() {
        assertEquals("1 sat", formatScaledSats(1000))
    }

    @Test
    fun formatScaledSats_500() {
        assertEquals("0.5 sat", formatScaledSats(500))
    }

    @Test
    fun formatScaledSats_with_unit() {
        assertEquals("1 sat/MB", formatScaledSats(1000, "MB"))
    }

    @Test
    fun stripScheme_https() {
        assertEquals("mint.example", stripScheme("https://mint.example"))
    }

    @Test
    fun stripScheme_http() {
        assertEquals("mint.example", stripScheme("http://mint.example"))
    }

    @Test
    fun stripScheme_no_scheme() {
        assertEquals("mint.example", stripScheme("mint.example"))
    }
}