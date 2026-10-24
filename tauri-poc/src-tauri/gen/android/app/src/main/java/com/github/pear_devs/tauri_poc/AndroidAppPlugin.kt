package com.github.pear_devs.tauri_poc

import android.app.Activity
import android.content.ContentUris
import android.content.ContentValues
import android.content.Intent
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Process
import android.provider.MediaStore
import android.text.InputType
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView
import androidx.annotation.RequiresApi
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
import java.io.File
import java.io.IOException
import org.json.JSONArray
import org.json.JSONObject

@InvokeArg
class FullscreenArgs {
  var fullscreen: Boolean = false
}

@InvokeArg
class AudioArgs {
  var path: String? = null
  lateinit var name: String
  var subfolder: String? = null
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

  // The downloader's files (src/plugins/downloader.rs). Android 10+ puts them
  // in the shared Music/Pear Desktop collection through MediaStore, which
  // needs no storage permission. Android 8 and 9 would need one, so there
  // they go to the app's own Music folder (removed with the app).

  @RequiresApi(Build.VERSION_CODES.Q)
  private fun relativePath(subfolder: String?) =
    listOfNotNull(Environment.DIRECTORY_MUSIC, "Pear Desktop", subfolder).joinToString("/") + "/"

  @RequiresApi(Build.VERSION_CODES.Q)
  private fun audioCollection() = MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)

  /** Android 10+: this app's file of that name in the collection. */
  @RequiresApi(Build.VERSION_CODES.Q)
  private fun findAudio(name: String, subfolder: String?): Uri? {
    val collection = audioCollection()
    activity.contentResolver.query(
      collection,
      arrayOf(MediaStore.MediaColumns._ID),
      "${MediaStore.MediaColumns.RELATIVE_PATH} = ? AND ${MediaStore.MediaColumns.DISPLAY_NAME} = ?",
      arrayOf(relativePath(subfolder), name),
      null,
    )?.use { cursor ->
      if (cursor.moveToFirst()) return ContentUris.withAppendedId(collection, cursor.getLong(0))
    }
    return null
  }

  private fun legacyFile(name: String, subfolder: String?): File {
    val root = activity.getExternalFilesDir(Environment.DIRECTORY_MUSIC) ?: throw IOException("No music folder")
    return File(listOfNotNull("Pear Desktop", subfolder).fold(root) { dir, part -> File(dir, part) }, name)
  }

  /** Runs `work` off the UI thread and answers `{ value }`. */
  private fun answer(invoke: Invoke, work: () -> Any) {
    Thread {
      try {
        invoke.resolve(JSObject().put("value", work()))
      } catch (error: Exception) {
        invoke.reject(error.message ?: error.toString())
      }
    }.start()
  }

  @Command
  fun audioExists(invoke: Invoke) {
    val args = invoke.parseArgs(AudioArgs::class.java)
    answer(invoke) {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) findAudio(args.name, args.subfolder) != null
      else legacyFile(args.name, args.subfolder).exists()
    }
  }

  /** Copies the downloaded file in, replacing one of the same name. */
  @Command
  fun saveAudio(invoke: Invoke) {
    val args = invoke.parseArgs(AudioArgs::class.java)
    answer(invoke) {
      val source = File(args.path ?: throw IOException("No file"))
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        val resolver = activity.contentResolver
        val existing = findAudio(args.name, args.subfolder)
        val uri = existing ?: resolver.insert(
          audioCollection(),
          ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, args.name)
            put(MediaStore.MediaColumns.MIME_TYPE, "audio/mp4")
            put(MediaStore.MediaColumns.RELATIVE_PATH, relativePath(args.subfolder))
            put(MediaStore.MediaColumns.IS_PENDING, 1)
          },
        ) ?: throw IOException("MediaStore did not accept ${args.name}")
        try {
          val output = resolver.openOutputStream(uri, "wt") ?: throw IOException("Cannot write ${args.name}")
          output.use { out -> source.inputStream().use { it.copyTo(out) } }
        } catch (error: Exception) {
          if (existing == null) resolver.delete(uri, null, null)
          throw error
        }
        resolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        uri.toString()
      } else {
        val target = legacyFile(args.name, args.subfolder)
        target.parentFile?.mkdirs()
        source.copyTo(target, overwrite = true)
        MediaScannerConnection.scanFile(activity, arrayOf(target.path), arrayOf("audio/mp4"), null)
        Uri.fromFile(target).toString()
      }
    }
  }
}
