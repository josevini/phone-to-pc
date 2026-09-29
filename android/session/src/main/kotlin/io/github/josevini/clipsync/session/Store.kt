package io.github.josevini.clipsync.session

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.nio.file.Files
import java.nio.file.StandardCopyOption

@Serializable
data class SavedAddress(
    val ip: String,
    val port: Int,
)

/** A paired device and the addresses it was reached at, dialed again after a restart. */
@Serializable
data class SavedDevice(
    val id: String,
    val name: String,
    val addrs: List<SavedAddress> = emptyList(),
)

/** What a node keeps between runs: its paired devices and Lamport counter (spec §6). */
@Serializable
data class SavedState(
    val lamport: Long = 0,
    val paired: List<SavedDevice> = emptyList(),
)

interface StateStore {
    fun load(): SavedState

    fun save(state: SavedState)
}

/** JSON in one file, replaced atomically on every save. A damaged file is an error, not an empty state. */
class FileStateStore(
    private val file: File,
) : StateStore {
    private val json =
        Json {
            ignoreUnknownKeys = true
            prettyPrint = true
        }

    override fun load(): SavedState = if (file.exists()) json.decodeFromString(file.readText()) else SavedState()

    override fun save(state: SavedState) {
        file.parentFile?.mkdirs()
        val temporary = File(file.path + ".tmp")
        temporary.writeText(json.encodeToString(state))
        Files.move(temporary.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
    }
}
