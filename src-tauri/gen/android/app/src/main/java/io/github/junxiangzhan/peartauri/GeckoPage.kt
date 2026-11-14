package io.github.junxiangzhan.peartauri

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import org.json.JSONObject
import org.mozilla.geckoview.AllowOrDeny
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoRuntimeSettings
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSessionSettings
import org.mozilla.geckoview.GeckoView

/**
 * YouTube Music in GeckoView (Gecko, the Firefox engine) instead of Tauri's
 * WebView, so Google sign-in works (GECKOVIEW-PLAN.md). MainActivity shows
 * [view] over the blank Tauri WebView. The built-in extension
 * (assets/pear-gecko) loads the renderer bundle before the page's scripts
 * and connects it to Rust through [GeckoBridge]; the page is loaded once its
 * background script has registered the loader with Rust's config.
 */
class GeckoPage(private val activity: Activity) {
  val view = GeckoView(activity)
  private val session = GeckoSession(
    // Desktop Firefox: the renderer plugins need the desktop site. The
    // viewport follows the page's meta tag (width=device-width), like the
    // WebView: the desktop viewport mode lays pages out at least 980 CSS px
    // wide, so narrow windows never got YTM's narrow layouts
    GeckoSessionSettings.Builder()
      .userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_DESKTOP)
      .viewportMode(GeckoSessionSettings.VIEWPORT_MODE_MOBILE)
      .build()
  )
  var canGoBack = false
    private set
  private var ready = false
  /** A link opened before the page was ready; it replaces the start URL. */
  private var pending: String? = null
  private val reportFile = java.io.File(activity.filesDir, "gecko-report.json")
  private val runtime = runtime(activity)

  init {
    session.navigationDelegate = object : GeckoSession.NavigationDelegate {
      override fun onCanGoBack(session: GeckoSession, canGoBack: Boolean) {
        this@GeckoPage.canGoBack = canGoBack
      }

      // Also for YTM's in-page navigation (history.pushState)
      override fun onLocationChange(
        session: GeckoSession,
        url: String?,
        perms: MutableList<GeckoSession.PermissionDelegate.ContentPermission>,
        hasUserGesture: Boolean,
      ) {
        GeckoBridge.pageUrl = url
      }

      // A new window (window.open, a target=_blank link): the system's
      // browser, as Pear's desktop window opens links outside itself
      override fun onLoadRequest(
        session: GeckoSession,
        request: GeckoSession.NavigationDelegate.LoadRequest,
      ): GeckoResult<AllowOrDeny>? {
        if (request.target != GeckoSession.NavigationDelegate.TARGET_WINDOW_NEW) return null
        openOutside(request.uri)
        return GeckoResult.fromValue(AllowOrDeny.DENY)
      }

      override fun onNewSession(session: GeckoSession, uri: String): GeckoResult<GeckoSession>? {
        openOutside(uri)
        return null
      }
    }
    // Let the page start audio without a gesture, like the WebView build
    // (mediaPlaybackRequiresUserGesture = false); YTM autoplays on resume
    session.permissionDelegate = object : GeckoSession.PermissionDelegate {
      override fun onContentPermissionRequest(
        session: GeckoSession,
        perm: GeckoSession.PermissionDelegate.ContentPermission,
      ): GeckoResult<Int> {
        val autoplay = perm.permission == GeckoSession.PermissionDelegate.PERMISSION_AUTOPLAY_AUDIBLE ||
          perm.permission == GeckoSession.PermissionDelegate.PERMISSION_AUTOPLAY_INAUDIBLE
        return GeckoResult.fromValue(
          if (autoplay) GeckoSession.PermissionDelegate.ContentPermission.VALUE_ALLOW
          else GeckoSession.PermissionDelegate.ContentPermission.VALUE_PROMPT
        )
      }
    }
    // Leave without asking, like Electron's will-prevent-unload handler in
    // src/index.ts: YTM asks while playing, and with no prompt shown the
    // reloads from the menu were refused
    session.promptDelegate = object : GeckoSession.PromptDelegate {
      override fun onBeforeUnloadPrompt(
        session: GeckoSession,
        prompt: GeckoSession.PromptDelegate.BeforeUnloadPrompt,
      ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> =
        GeckoResult.fromValue(prompt.confirm(AllowOrDeny.ALLOW))

      // <input type=file> (uploading songs): the system's document picker
      override fun onFilePrompt(
        session: GeckoSession,
        prompt: GeckoSession.PromptDelegate.FilePrompt,
      ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> = pickFiles(prompt)
    }
    // Android kills Gecko's content processes when memory runs low, mostly
    // while the app sits in the background. The session is closed then, and
    // coming back showed a white page until the app was restarted. Reopen it
    // and load the page again, where the user was.
    session.contentDelegate = object : GeckoSession.ContentDelegate {
      override fun onKill(session: GeckoSession) = recover("was killed")
      override fun onCrash(session: GeckoSession) = recover("crashed")
    }
    session.open(runtime)
    view.setSession(session)

    runtime.webExtensionController
      .ensureBuiltIn(EXTENSION_URL, EXTENSION_ID)
      .accept({ extension ->
        Log.i(TAG, "extension installed: ${extension?.id}")
        if (extension == null) return@accept
        extension.setMessageDelegate(GeckoBridge.delegate(GeckoBridge.BACKGROUND, ::onBackgroundMessage), "pear")
        session.webExtensionController.setMessageDelegate(
          extension,
          GeckoBridge.delegate(GeckoBridge.PAGE, ::onPageMessage),
          "pear",
        )
        // Reopened while the process (and the background script) lived on
        loaderStartUrl?.let(::start)
      }, { error ->
        Log.e(TAG, "installing the extension failed", error)
      })
  }

  private fun onBackgroundMessage(message: JSONObject) {
    when {
      message.optBoolean("ready") -> {
        Log.i(TAG, "loader registered")
        val startUrl = message.optString("startUrl").ifEmpty { YTM_URL }
        loaderStartUrl = startUrl
        start(startUrl)
      }
      message.has("error") -> Log.e(TAG, "extension: ${message.optString("error")}")
    }
  }

  private fun start(startUrl: String) {
    if (ready) return
    ready = true
    session.loadUri(pending ?: startUrl)
    pending = null
  }

  private fun openOutside(uri: String) {
    try {
      activity.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(uri)))
    } catch (error: ActivityNotFoundException) {
      Log.w(TAG, "no app opens $uri", error)
    }
  }

  /**
   * The picked documents, copied into the cache: Gecko reads files, and a
   * picker's content:// URIs are not files
   */
  private fun pickFiles(
    prompt: GeckoSession.PromptDelegate.FilePrompt,
  ): GeckoResult<GeckoSession.PromptDelegate.PromptResponse> {
    val registry = (activity as? ComponentActivity)?.activityResultRegistry
      ?: return GeckoResult.fromValue(prompt.dismiss())
    val result = GeckoResult<GeckoSession.PromptDelegate.PromptResponse>()
    val types = prompt.mimeTypes?.filter { it.contains('/') }?.toTypedArray()
      ?.takeIf { it.isNotEmpty() } ?: arrayOf("*/*")
    var launcher: ActivityResultLauncher<Array<String>>? = null
    fun finish(uris: List<Uri>) {
      launcher?.unregister()
      if (uris.isEmpty()) {
        result.complete(prompt.dismiss())
        return
      }
      // One directory per pick, and one per file in it: two documents with
      // the same name must not overwrite each other, and the name stays as
      // it is, since the page uploads it as the file's name
      val root = java.io.File(activity.cacheDir, "picked")
      val batch = java.io.File(root, "${System.currentTimeMillis()}-${++picks}")
      Thread {
        // Earlier picks may still be uploading (the page reads the file as
        // it sends it), so only old ones are cleared
        val stale = System.currentTimeMillis() - PICKED_MAX_AGE_MS
        root.listFiles()
          ?.filter { it != batch && it.lastModified() < stale }
          ?.forEach { it.deleteRecursively() }
        val files = uris.mapIndexedNotNull { index, uri ->
          try {
            val name = displayName(uri) ?: "file-$index"
            val file = java.io.File(batch, "$index/$name").apply { parentFile?.mkdirs() }
            activity.contentResolver.openInputStream(uri)?.use { input ->
              file.outputStream().use { input.copyTo(it) }
            } ?: return@mapIndexedNotNull null
            Uri.fromFile(file)
          } catch (error: Exception) {
            Log.w(TAG, "reading $uri failed", error)
            null
          }
        }
        activity.runOnUiThread {
          result.complete(
            if (files.isEmpty()) prompt.dismiss()
            else prompt.confirm(activity, files.toTypedArray())
          )
        }
      }.start()
    }
    launcher = if (prompt.type == GeckoSession.PromptDelegate.FilePrompt.Type.MULTIPLE) {
      registry.register(FILE_PROMPT_KEY, ActivityResultContracts.OpenMultipleDocuments()) { finish(it) }
    } else {
      registry.register(FILE_PROMPT_KEY, ActivityResultContracts.OpenDocument()) { finish(listOfNotNull(it)) }
    }
    try {
      launcher.launch(types)
    } catch (error: ActivityNotFoundException) {
      // No document picker on the device
      Log.w(TAG, "no app picks documents", error)
      launcher.unregister()
      result.complete(prompt.dismiss())
    }
    return result
  }

  private fun displayName(uri: Uri): String? =
    activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
      ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
      // A name, not a path
      ?.substringAfterLast('/')

  private fun recover(what: String) {
    Log.w(TAG, "content process $what; reloading the page")
    if (!session.isOpen) session.open(runtime)
    if (ready) session.loadUri(GeckoBridge.pageUrl ?: loaderStartUrl ?: YTM_URL)
  }

  // report.js: diagnostics, too long for logcat; the latest one is kept
  private fun onPageMessage(message: JSONObject) {
    reportFile.writeText(message.toString())
    Log.i(TAG, "report: ${message.toString().take(300)}")
  }

  fun open(url: String) {
    if (ready) session.loadUri(url) else pending = url
  }

  fun goBack() = session.goBack()

  fun close() = session.close()

  companion object {
    private const val TAG = "PearGecko"
    private const val YTM_URL = "https://music.youtube.com/"
    private const val FILE_PROMPT_KEY = "pear-file-prompt"
    /** How long picked files stay in the cache: past any upload of them. */
    private const val PICKED_MAX_AGE_MS = 24 * 60 * 60 * 1000L
    /** Names each pick's cache directory, with the time. */
    private var picks = 0
    private const val EXTENSION_ID = "pear-gecko@pear-devs.github.io"
    private const val EXTENSION_URL = "resource://android/assets/pear-gecko/"
    private var sharedRuntime: GeckoRuntime? = null
    /** Set once the background script registered the loader. */
    private var loaderStartUrl: String? = null

    /** One GeckoRuntime per process, as GeckoView requires. */
    private fun runtime(activity: Activity): GeckoRuntime =
      sharedRuntime ?: GeckoRuntime.create(
        activity.applicationContext,
        GeckoRuntimeSettings.Builder()
          .consoleOutput(true)
          .configFilePath(writeConfig(activity))
          .build(),
      ).also { sharedRuntime = it }

    /**
     * Gecko preferences, through GeckoView's config file. For background
     * playback on phones (a POCO F7 Pro):
     * - No utility and GPU processes: Gecko drops them to idle priority once
     *   the app is in the background, and the phone killed them within 30 s,
     *   taking the audio or video decoder along (YTM reset the player).
     *   Their work then runs in processes that stay alive: the content
     *   process playing the audio, and this one (PlaybackService).
     * - Autoplay allowed, including Web Audio: the renderer plays the video
     *   through an AudioContext, which otherwise stayed suspended until the
     *   first tap. Its output was silence, and MIUI pauses an app's silent
     *   audio after 60 s in the background, which stopped playback.
     */
    private fun writeConfig(activity: Activity): String {
      val file = java.io.File(activity.filesDir, "geckoview-config.yaml")
      file.writeText(
        "prefs:\n" +
          "  media.utility-process.enabled: false\n" +
          "  layers.gpu-process.enabled: false\n" +
          "  media.autoplay.default: 0\n"
      )
      return file.absolutePath
    }
  }
}
