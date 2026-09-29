package io.github.josevini.clipsync

import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.MultiFormatReader
import com.google.zxing.NotFoundException
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer

private val hints =
    mapOf(
        DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
        DecodeHintType.TRY_HARDER to true,
        // `clipsync pair` draws its code light on dark. MultiFormatReader honours this; QRCodeReader alone does not.
        DecodeHintType.ALSO_INVERTED to true,
    )

/** Reads a QR code from a camera frame's luminance plane (row stride [rowStride]), or null if there is none. */
fun decodeQr(
    luminance: ByteArray,
    width: Int,
    height: Int,
    rowStride: Int,
): String? {
    val source = PlanarYUVLuminanceSource(luminance, rowStride, height, 0, 0, width, height, false)
    return try {
        MultiFormatReader().decode(BinaryBitmap(HybridBinarizer(source)), hints).text
    } catch (_: NotFoundException) {
        null
    } catch (_: ReaderException) {
        null
    }
}
