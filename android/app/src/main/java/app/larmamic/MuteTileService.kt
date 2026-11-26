package app.larmamic

import android.content.Intent
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.launch

/** Quick Settings tile: tap to mute / unmute the microphone while streaming. */
class MuteTileService : TileService() {
    private var scope: CoroutineScope? = null

    override fun onStartListening() {
        super.onStartListening()
        scope = CoroutineScope(SupervisorJob() + Dispatchers.Main).also { s ->
            s.launch { Engine.params.collect { refresh() } }
        }
    }

    override fun onStopListening() {
        scope?.cancel()
        scope = null
        super.onStopListening()
    }

    override fun onClick() {
        if (Engine.params.value == null) return
        startService(Intent(this, MicService::class.java).setAction(MicService.ACTION_TOGGLE_MUTE))
    }

    private fun refresh() {
        val tile = qsTile ?: return
        val p = Engine.params.value
        tile.label = getString(R.string.tile_label)
        tile.state = when {
            p == null -> Tile.STATE_UNAVAILABLE
            p["mute"] == true -> Tile.STATE_ACTIVE // muted = highlighted
            else -> Tile.STATE_INACTIVE
        }
        tile.updateTile()
    }
}
