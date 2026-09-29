package io.github.josevini.clipsync

import io.github.josevini.clipsync.session.Node
import io.github.josevini.clipsync.session.NodeEvent
import io.github.josevini.clipsync.session.NodeStatus
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/** The running node, as the UI sees it: its status and events. [SyncService] owns the node's lifecycle. */
object Sync {
    private val statusFlow = MutableStateFlow<NodeStatus?>(null)
    private val eventFlow = MutableSharedFlow<NodeEvent>(extraBufferCapacity = 64)

    /** Null while the service is not running. */
    val status: StateFlow<NodeStatus?> = statusFlow
    val events: SharedFlow<NodeEvent> = eventFlow

    @Volatile var node: Node? = null
        private set

    internal fun attach(node: Node) {
        this.node = node
        statusFlow.value = node.status()
    }

    internal fun detach() {
        node = null
        statusFlow.value = null
    }

    /** Called on the node's thread, where [Node.status] answers without waiting. */
    internal fun publish(
        node: Node,
        event: NodeEvent,
    ) {
        eventFlow.tryEmit(event)
        if (this.node === node) statusFlow.value = node.status()
    }
}
