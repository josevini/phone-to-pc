package io.github.josevini.clipsync

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class QrDecoderTest {
    private val uri =
        "clipsync://pair?v=1&id=86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027&name=book2" +
            "&addr=192.168.0.10:47823&token=000102030405060708090a0b0c0d0e0f"

    /** A camera frame's luminance plane: the code drawn dark on light, with [padding] extra bytes per row. */
    private fun frame(
        text: String,
        size: Int,
        padding: Int,
        inverted: Boolean = false,
    ): Triple<ByteArray, Int, Int> {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, size, size)
        val stride = size + padding
        val plane = ByteArray(stride * size)
        for (y in 0 until size) {
            for (x in 0 until size) {
                val dark = matrix[x, y] != inverted
                plane[y * stride + x] = if (dark) 0 else 255.toByte()
            }
        }
        return Triple(plane, stride, size)
    }

    @Test
    fun `a pairing code is read from a camera frame`() {
        val (plane, stride, size) = frame(uri, 400, padding = 16)
        assertEquals(uri, decodeQr(plane, size, size, stride))
    }

    @Test
    fun `a code drawn light on dark is read too`() {
        // `clipsync pair` draws its code light on dark, for dark terminals.
        val (plane, stride, size) = frame(uri, 400, padding = 0, inverted = true)
        assertEquals(uri, decodeQr(plane, size, size, stride))
    }

    @Test
    fun `a frame without a code is null`() {
        val plane = ByteArray(200 * 200) { 128.toByte() }
        assertNull(decodeQr(plane, 200, 200, 200))
    }
}
