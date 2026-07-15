package org.opentollgate.android.util

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.assertNull
import kotlin.test.assertNotNull
import kotlin.test.assertFailsWith
import java.util.Base64

class CashuTokenTest {
    private fun rawToken(json: String): String {
        val b64 = Base64.getUrlEncoder().withoutPadding().encodeToString(json.toByteArray(Charsets.UTF_8))
        return "cashuA$b64"
    }

    @Test
    fun parse_cashuA_valid() {
        val json = """{"token":[{"mint":"https://mint.example","proofs":[{"amount":21,"id":"009a1f293253e41e","secret":"abc123","C":"deadbeef"}]}],"unit":"sat"}"""
        val token = rawToken(json)
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok, "expected Ok, got $result")
        val ok = result as TokenResult.Ok
        assertEquals(21, ok.token.amountSat)
        assertEquals("https://mint.example", ok.token.singleMint)
        assertEquals("sat", ok.token.unit)
    }

    @Test
    fun parse_cashuA_multiple_mints() {
        val json = """{"token":[{"mint":"https://mint1.example","proofs":[{"amount":10,"id":"id1","secret":"s1","C":"c1"}]},{"mint":"https://mint2.example","proofs":[{"amount":11,"id":"id2","secret":"s2","C":"c2"}]}],"unit":"sat"}"""
        val token = rawToken(json)
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok)
        val ok = result as TokenResult.Ok
        assertEquals(2, ok.token.groups.size)
        assertEquals(21, ok.token.amountSat)
        assertNull(ok.token.singleMint, "singleMint should be null when multiple mints")
    }

    @Test
    fun parse_invalid_returns_error() {
        val result = parseCashuToken("not-a-cashu-token")
        assertTrue(result is TokenResult.Error)
    }

    @Test
    fun parse_empty_returns_error() {
        val result = parseCashuToken("")
        assertTrue(result is TokenResult.Error)
    }

    @Test
    fun parse_whitespace_trimmed() {
        val json = """{"token":[{"mint":"https://mint.example","proofs":[{"amount":1,"id":"id","secret":"s","C":"c"}]}],"unit":"sat"}"""
        val token = "  " + rawToken(json) + "  "
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok, "trimmed token should parse, got $result")
    }

    @Test
    fun parse_memo_preserved() {
        val json = """{"token":[{"mint":"https://mint.example","proofs":[{"amount":1,"id":"id","secret":"s","C":"c"}]}],"unit":"sat","memo":"test memo"}"""
        val token = rawToken(json)
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok)
        val ok = result as TokenResult.Ok
        assertNotNull(ok.token.memo)
        assertEquals("test memo", ok.token.memo)
    }

    @Test
    fun buildLocalToken_valid_cashuA() {
        val token = buildLocalToken("https://mint.example", 21)
        assertTrue(token.startsWith("cashuA"), "token should start with cashuA, got: ${token.take(10)}")
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok, "buildLocalToken output should parse, got $result")
        val ok = result as TokenResult.Ok
        assertEquals(21, ok.token.amountSat)
    }

    @Test
    fun buildLocalToken_zero_throws() {
        assertFailsWith<IllegalArgumentException> {
            buildLocalToken("https://mint.example", 0)
        }
    }

    @Test
    fun roundtrip_buildLocalToken_parseCashuToken() {
        for (amount in listOf(1L, 21L, 1000L)) {
            val token = buildLocalToken("https://mint.example", amount)
            val result = parseCashuToken(token)
            assertTrue(result is TokenResult.Ok, "amount $amount failed to round-trip: $result")
            assertEquals(amount, (result as TokenResult.Ok).token.amountSat)
        }
    }

    @Test
    fun amountSat_sums_groups() {
        val json = """{"token":[{"mint":"https://mint1.example","proofs":[{"amount":10,"id":"id1","secret":"s1","C":"c1"}]},{"mint":"https://mint2.example","proofs":[{"amount":11,"id":"id2","secret":"s2","C":"c2"}]}],"unit":"sat"}"""
        val token = rawToken(json)
        val result = parseCashuToken(token)
        assertTrue(result is TokenResult.Ok)
        assertEquals(21, (result as TokenResult.Ok).token.amountSat)
    }
}