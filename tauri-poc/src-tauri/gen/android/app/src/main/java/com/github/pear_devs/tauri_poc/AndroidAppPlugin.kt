package com.github.pear_devs.tauri_poc

import android.app.Activity
import android.content.Intent
import android.os.Process
import android.text.InputType
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject

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

  /**
   * A dialog for `custom-electron-prompt` options, answering like
   * `placeholder/prompt.html`: the text for "input", the number for
   * "counter", the option key for "select", a list for "multiInput", and
   * null when cancelled. Keybind prompts don't apply on Android.
   */
  @Command
  fun prompt(invoke: Invoke) {
    val options = invoke.getArgs()
    activity.runOnUiThread {
      val fields = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        val padding = (20 * activity.resources.displayMetrics.density).toInt()
        setPadding(padding, padding / 2, padding, 0)
      }
      val label = options.optString("label")
      if (label.isNotEmpty()) fields.addView(TextView(activity).apply { text = label })
      fun textField(value: String, attrs: JSONObject?) = EditText(activity).apply {
        setText(value)
        hint = attrs?.optString("placeholder").orEmpty()
        inputType = when (attrs?.optString("type")) {
          "number" -> InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_SIGNED
          "url" -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
          "password" -> InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
          else -> InputType.TYPE_CLASS_TEXT
        }
        fields.addView(this)
      }
      val value = options.opt("value")?.takeIf { it != JSONObject.NULL }?.toString().orEmpty()
      val read: () -> Any = when (options.optString("type", "input")) {
        "input" -> {
          val field = textField(value, options.optJSONObject("inputAttrs"))
          val reader: () -> Any = { field.text.toString() }
          reader
        }
        "counter" -> {
          val limits = options.optJSONObject("counterOptions")
          val field = textField(value.ifEmpty { "0" }, JSONObject().put("type", "number"))
          val clamp = { number: Double ->
            var result = number
            limits?.optDouble("minimum")?.takeIf { !it.isNaN() }?.let { result = maxOf(result, it) }
            limits?.optDouble("maximum")?.takeIf { !it.isNaN() }?.let { result = minOf(result, it) }
            result
          }
          val reader: () -> Any = {
            clamp(field.text.toString().toDoubleOrNull() ?: 0.0).let { if (it % 1.0 == 0.0) it.toLong() else it }
          }
          reader
        }
        "select" -> {
          val choices = options.optJSONObject("selectOptions") ?: JSONObject()
          val keys = choices.keys().asSequence().toList()
          val group = RadioGroup(activity)
          keys.forEachIndexed { index, key ->
            group.addView(RadioButton(activity).apply {
              id = index + 1
              text = choices.optString(key)
              isChecked = key == value
            })
          }
          fields.addView(group)
          val reader: () -> Any = { keys.getOrNull(group.checkedRadioButtonId - 1) ?: value }
          reader
        }
        "multiInput" -> {
          val inputs = options.optJSONArray("multiInputOptions") ?: JSONArray()
          val values = options.optJSONArray("value")
          val texts = (0 until inputs.length()).map { index ->
            val input = inputs.optJSONObject(index) ?: JSONObject()
            val caption = input.optString("label")
            if (caption.isNotEmpty()) fields.addView(TextView(activity).apply { text = caption })
            val initial = values?.opt(index)?.takeIf { it != JSONObject.NULL }?.toString()
              ?: input.optJSONObject("inputAttrs")?.optString("value").orEmpty()
            textField(initial, input.optJSONObject("inputAttrs"))
          }
          val reader: () -> Any = { JSONArray(texts.map { it.text.toString() }) }
          reader
        }
        else -> return@runOnUiThread invoke.reject("${options.optString("type")} prompts are not available on Android")
      }
      var answered = false
      AlertDialog.Builder(activity)
        .setTitle(options.optString("title"))
        .setView(ScrollView(activity).apply { addView(fields) })
        .setPositiveButton(android.R.string.ok) { _, _ ->
          answered = true
          invoke.resolve(JSObject().put("value", read()))
        }
        .setNegativeButton(android.R.string.cancel, null)
        .setOnDismissListener { if (!answered) invoke.resolve(JSObject().put("value", JSONObject.NULL)) }
        .show()
    }
  }
}
