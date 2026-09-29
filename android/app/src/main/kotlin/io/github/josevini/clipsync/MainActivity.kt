package io.github.josevini.clipsync

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import io.github.josevini.clipsync.ui.App
import io.github.josevini.clipsync.ui.ClipsyncTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        SyncService.start(this)
        setContent { ClipsyncTheme { App() } }
    }
}
