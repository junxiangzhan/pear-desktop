package com.github.pear_devs.tauri_poc

import android.app.Activity
import android.util.Log
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
class GeckoPage(activity: Activity) {
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
    }
    val runtime = runtime(activity)
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
          .build(),
      ).also { sharedRuntime = it }
  }
}
