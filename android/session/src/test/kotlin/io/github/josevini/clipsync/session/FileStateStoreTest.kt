package io.github.josevini.clipsync.session

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.File
import kotlin.io.path.createTempDirectory

class FileStateStoreTest {
    private val dir = createTempDirectory("clipsync-store").toFile()
    private val file = File(dir, "state.json")

    @Test
    fun `a missing file is an empty state`() {
        assertEquals(SavedState(), FileStateStore(file).load())
    }

    @Test
    fun `the state round-trips`() {
        val state =
            SavedState(
                lamport = 42,
                paired = listOf(SavedDevice("ab".repeat(32), "Meu PC", listOf(SavedAddress("fd00::2", 47823)))),
            )
        FileStateStore(file).save(state)
        assertEquals(state, FileStateStore(file).load())
        assertEquals(listOf("state.json"), dir.list()!!.toList())
    }

    @Test
    fun `a damaged file is an error, not an empty state`() {
        file.writeText("{not json")
        assertThrows(Exception::class.java) { FileStateStore(file).load() }
    }
}
