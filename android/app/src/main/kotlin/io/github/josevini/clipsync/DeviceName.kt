package io.github.josevini.clipsync

import android.content.Context
import android.os.Build
import androidx.core.content.edit
import io.github.josevini.clipsync.core.isValidName

private const val PREFS = "settings"
private const val KEY_NAME = "device_name"
private const val MAX_NAME_BYTES = 64

/** The default device name: the phone's model, cut to the protocol's 64 bytes. */
fun defaultDeviceName(model: String): String {
    var name = model.trim()
    while (name.encodeToByteArray().size > MAX_NAME_BYTES) name = name.dropLast(1)
    return name.ifEmpty { "Android" }
}

/** The name other devices see, chosen by the user or [defaultDeviceName]. */
object DeviceName {
    fun get(context: Context): String =
        prefs(context).getString(KEY_NAME, null)?.takeIf { isValidName(it) } ?: defaultDeviceName(Build.MODEL ?: "")

    /** Saves [name] if it is valid (1–64 bytes); returns whether it was. */
    fun set(
        context: Context,
        name: String,
    ): Boolean {
        val trimmed = name.trim()
        if (!isValidName(trimmed)) return false
        prefs(context).edit { putString(KEY_NAME, trimmed) }
        return true
    }

    private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}
