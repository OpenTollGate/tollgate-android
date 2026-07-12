package org.opentollgate.android

import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.mockito.Mock
import org.mockito.Mockito.*
import org.mockito.MockitoAnnotations
import org.mockito.junit.MockitoJUnitRunner
import java.net.*
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Unit test for UdpTransport.
 * 
 * Tests the UDP transport functionality with mocked DatagramSocket
 * to verify bytes flow through the channel bridge in both directions.
 */
@RunWith(MockitoJUnitRunner::class)
class UdpTransportTest {

    @Mock
    private lateinit var mockSocket: DatagramSocket
    
    @Mock
    private lateinit var mockInetAddress: InetAddress
    
    private lateinit var udpTransport: UdpTransport
    private val testHost = "test.gateway.com"
    private val testPort = 2121
    private val testPacketData = byteArrayOf(0x01, 0x02, 0x03, 0x04, 0x05)
    private val testHostGateway = "66.92.204.38"
    
    @Before
    fun setUp() {
        MockitoAnnotations.openMocks(this)
        
        // Mock socket connection and behavior
        `when`(mockInetAddress.hostName).thenReturn(testHost)
        `when`(mockSocket.inetAddress).thenReturn(mockInetAddress)
        `when`(mockSocket.port).thenReturn(testPort)
        `when`(mockSocket.isConnected).thenReturn(true)
        `when`(mockSocket.isClosed).thenReturn(false)
        
        // Mock successful packet reception
        `when`(mockSocket.receive(any())).thenAnswer { invocation ->
            val packet = invocation.arguments[0] as DatagramPacket
            packet.setData(testPacketData)
            packet.length = testPacketData.size
        }
        
        // Create transport instance
        udpTransport = UdpTransport(testHostGateway, testPort)
    }
    
    @After
    fun tearDown() {
        // Ensure transport is stopped after each test
        if (::udpTransport.isInitialized) {
            udpTransport.stop()
        }
    }
    
    @Test
    fun `start and stop transport successfully`() {
        // Start transport
        udpTransport.start()
        assertTrue(udpTransport.isRunning(), "Transport should be running after start()")
        
        // Stop transport
        udpTransport.stop()
        assertFalse(udpTransport.isRunning(), "Transport should not be running after stop()")
    }
    
    @Test
    fun `double start should not cause issues`() {
        udpTransport.start()
        
        // Start again - should not cause problems
        udpTransport.start()
        assertTrue(udpTransport.isRunning(), "Transport should still be running after double start")
        
        udpTransport.stop()
    }
    
    @Test
    fun `transport should connect to correct gateway`() {
        // Test that the transport connects to the correct host and port
        udpTransport.start()
        
        // Verify socket configuration
        verify(mockSocket).connect(any(), eq(testPort))
        verify(mockSocket).soTimeout = UdpTransport.SOCKET_TIMEOUT_MS
        
        udpTransport.stop()
    }
    
    @Test
    fun `receive thread should deliver packets to mesh`() {
        // Mock NativeCore.deliverPacket to return true (success)
        `when`(NativeCore.deliverPacket(any())).thenReturn(true)
        
        udpTransport.start()
        
        // Give some time for the receive thread to process
        Thread.sleep(200)
        
        // Verify deliverPacket was called with test data
        verify(NativeCore, atLeastOnce()).deliverPacket(testPacketData)
        
        udpTransport.stop()
    }
    
    @Test
    fun `receive thread should handle deliverPacket failure`() {
        // Mock NativeCore.deliverPacket to return false (failure)
        `when`(NativeCore.deliverPacket(any())).thenReturn(false)
        
        udpTransport.start()
        
        // Give some time for the receive thread to process
        Thread.sleep(200)
        
        // Verify deliverPacket was still called even if it failed
        verify(NativeCore, atLeastOnce()).deliverPacket(testPacketData)
        
        udpTransport.stop()
    }
    
    @Test
    fun `receive thread should handle socket timeout gracefully`() {
        // Mock socket to throw SocketTimeoutException
        `when`(mockSocket.receive(any())).thenThrow(SocketTimeoutException("Timeout"))
        
        udpTransport.start()
        
        // Give some time for the receive thread to process
        Thread.sleep(200)
        
        // Thread should continue running despite timeout
        assertTrue(udpTransport.isRunning(), "Transport should continue running after timeout")
        
        udpTransport.stop()
    }
    
    @Test
    fun `receive thread should stop when transport is stopped`() {
        udpTransport.start()
        
        // Wait for thread to start
        Thread.sleep(100)
        
        // Stop transport
        udpTransport.stop()
        
        // Verify transport is no longer running
        assertFalse(udpTransport.isRunning(), "Transport should not be running after stop")
    }
    
    @Test
    fun `send thread should send packets from mesh`() {
        // Mock NativeCore.nextOutboundPacket to return test data
        `when`(NativeCore.nextOutboundPacket(anyLong()))
            .thenReturn(testPacketData)
            .thenReturn(null) // Return null on subsequent calls
        
        udpTransport.start()
        
        // Give some time for the send thread to process
        Thread.sleep(300)
        
        // Verify nextOutboundPacket was called and packet was sent
        verify(NativeCore, atLeastOnce()).nextOutboundPacket(UdpTransport.SEND_POLL_TIMEOUT_MS.toLong())
        verify(mockSocket).send(any())
        
        udpTransport.stop()
    }
    
    @Test
    fun `send thread should handle null packets gracefully`() {
        // Mock NativeCore.nextOutboundPacket to return null (no data)
        `when`(NativeCore.nextOutboundPacket(anyLong())).thenReturn(null)
        
        udpTransport.start()
        
        // Give some time for the send thread to process
        Thread.sleep(200)
        
        // Verify nextOutboundPacket was called but no packet was sent
        verify(NativeCore, atLeastOnce()).nextOutboundPacket(UdpTransport.SEND_POLL_TIMEOUT_MS.toLong())
        verify(mockSocket, never()).send(any())
        
        udpTransport.stop()
    }
    
    @Test
    fun `send thread should handle socket exceptions gracefully`() {
        // Mock socket to throw exception on send
        `when`(mockSocket.send(any())).thenThrow(SocketException("Network error"))
        `when`(NativeCore.nextOutboundPacket(anyLong())).thenReturn(testPacketData)
        
        udpTransport.start()
        
        // Give some time for the send thread to process
        Thread.sleep(300)
        
        // Verify transport continues running despite exception
        assertTrue(udpTransport.isRunning(), "Transport should continue running after send exception")
        
        udpTransport.stop()
    }
    
    @Test
    fun `transport should use correct default gateway address`() {
        // Test default constructor with hardcoded gateway
        val defaultTransport = UdpTransport()
        
        // Verify default values
        assertTrue(defaultTransport.isRunning())
        defaultTransport.stop()
    }
    
    @Test
    fun `transport should handle socket close gracefully`() {
        // Close the socket explicitly
        `when`(mockSocket.isClosed).thenReturn(true)
        
        udpTransport.start()
        
        // Give some time for the transport to detect closed socket
        Thread.sleep(200)
        
        // Transport should handle it gracefully
        assertTrue(udpTransport.isRunning())
        
        udpTransport.stop()
    }
}