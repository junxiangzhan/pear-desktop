package com.github.pear_devs.tauri_poc

import android.app.Activity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ActionChannelArgs {
  lateinit var channel: Channel
}

@InvokeArg
class UpdateArgs {
  var title: String = ""
  var artist: String = ""
  var album: String? = null
  var artworkUrl: String? = null
  var isPaused: Boolean = true
  var durationMs: Long = 0
  var positionMs: Long = 0
}

/**
 * Commands for `src/android_media.rs`: the song to show in the media
 * session, and a channel for the controls pressed there.
 */
@TauriPlugin
class MediaSessionPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun setActionChannel(invoke: Invoke) {
    val channel = invoke.parseArgs(ActionChannelArgs::class.java).channel
    PlaybackService.actions = { action, value ->
      channel.send(JSObject().apply {
        put("action", action)
        put("value", value)
      })
    }
    invoke.resolve()
  }

  @Command
  fun update(invoke: Invoke) {
    val args = invoke.parseArgs(UpdateArgs::class.java)
    PlaybackService.update(
      activity,
      PlaybackInfo(args.title, args.artist, args.album, args.artworkUrl, args.isPaused, args.durationMs, args.positionMs),
    )
    invoke.resolve()
  }

  @Command
  fun stop(invoke: Invoke) {
    PlaybackService.stop(activity)
    invoke.resolve()
  }
}
