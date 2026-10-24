package com.github.pear_devs.tauri_poc

import android.app.Activity
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class FullscreenArgs {
  var fullscreen: Boolean = false
}

/**
 * Full screen for `src/system_bars.rs`: hides the status bar and the
 * navigation bar (three buttons or the gesture handle). Swiping from an edge
 * shows them over the page for a moment. MainActivity's insets padding
 * drops to zero while they are hidden, so the page fills the screen.
 */
@TauriPlugin
class SystemBarsPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun setFullscreen(invoke: Invoke) {
    val fullscreen = invoke.parseArgs(FullscreenArgs::class.java).fullscreen
    activity.runOnUiThread {
      val controller = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
      controller.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      if (fullscreen) controller.hide(WindowInsetsCompat.Type.systemBars())
      else controller.show(WindowInsetsCompat.Type.systemBars())
    }
    invoke.resolve()
  }
}
