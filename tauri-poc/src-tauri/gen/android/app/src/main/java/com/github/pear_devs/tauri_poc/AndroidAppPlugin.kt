package com.github.pear_devs.tauri_poc

import android.app.Activity
import android.content.Intent
import android.os.Process
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

/** App-level commands for `src/android_app.rs`. */
@TauriPlugin
class AndroidAppPlugin(private val activity: Activity) : Plugin(activity) {
  /**
   * Hides the status bar and the navigation bar (three buttons or the gesture
   * handle). Swiping from an edge shows them over the page for a moment.
   * MainActivity's insets padding drops to zero while they are hidden, so the
   * page fills the screen.
   */
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

  /**
   * Starts the app again in a new process. The launch goes through the
   * system, which starts a fresh process for it once this one is gone; the
   * process is killed rather than exited, because exiting runs native
   * teardown while Gecko's audio thread is still running (it aborts).
   */
  @Command
  fun restart(invoke: Invoke) {
    invoke.resolve()
    activity.runOnUiThread {
      val launch = activity.packageManager.getLaunchIntentForPackage(activity.packageName)
      launch?.component?.let { activity.startActivity(Intent.makeRestartActivityTask(it)) }
      Process.killProcess(Process.myPid())
    }
  }
}
