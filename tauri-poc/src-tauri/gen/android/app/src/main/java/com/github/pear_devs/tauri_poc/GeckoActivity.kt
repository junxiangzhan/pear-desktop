package com.github.pear_devs.tauri_poc

import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.view.View
import androidx.activity.OnBackPressedCallback
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoRuntimeSettings
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSessionSettings
import org.mozilla.geckoview.GeckoView
import org.mozilla.geckoview.WebExtension

/**
 * GeckoView test bed, separate from the Tauri app (GECKOVIEW-PLAN.md):
 * YouTube Music in GeckoView (Gecko, the Firefox engine) instead of Android's
 * WebView, so Google sign-in works. The built-in extension (assets/pear-gecko)
 * loads the renderer bundle before the page's scripts; in phase 1 a stub
 * answers its IPC instead of Rust. Reports go to logcat (tag PearGecko) and
 * files/gecko-report.json.
 */
class GeckoActivity : AppCompatActivity() {
  private lateinit var session: GeckoSession
  private var canGoBack = false

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    val view = GeckoView(this)
    view.setBackgroundColor(Color.rgb(3, 3, 3))
    setContentView(view)
    ViewCompat.setOnApplyWindowInsetsListener(view) { v: View, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      v.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }

    // Desktop Firefox: the renderer plugins need the desktop site
    session = GeckoSession(
      GeckoSessionSettings.Builder()
        .userAgentMode(GeckoSessionSettings.USER_AGENT_MODE_DESKTOP)
        .viewportMode(GeckoSessionSettings.VIEWPORT_MODE_DESKTOP)
        .build()
    )
    session.navigationDelegate = object : GeckoSession.NavigationDelegate {
      override fun onCanGoBack(session: GeckoSession, canGoBack: Boolean) {
        this@GeckoActivity.canGoBack = canGoBack
      }
    }
    // Test switch (phase 1): let the page start audio without a user
    // gesture, as the WebView build does (mediaPlaybackRequiresUserGesture)
    if (intent?.getBooleanExtra("allowAutoplay", false) == true) {
      session.permissionDelegate = object : GeckoSession.PermissionDelegate {
        override fun onContentPermissionRequest(
          session: GeckoSession,
          perm: GeckoSession.PermissionDelegate.ContentPermission,
        ): GeckoResult<Int> {
          val autoplay = perm.permission == GeckoSession.PermissionDelegate.PERMISSION_AUTOPLAY_AUDIBLE ||
            perm.permission == GeckoSession.PermissionDelegate.PERMISSION_AUTOPLAY_INAUDIBLE
          Log.i(TAG, "permission ${perm.permission} → ${if (autoplay) "allow" else "prompt"}")
          return GeckoResult.fromValue(
            if (autoplay) GeckoSession.PermissionDelegate.ContentPermission.VALUE_ALLOW
            else GeckoSession.PermissionDelegate.ContentPermission.VALUE_PROMPT
          )
        }
      }
    }
    val runtime = runtime(this)
    session.open(runtime)
    view.setSession(session)

    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        if (canGoBack) session.goBack() else finish()
      }
    })

    start = intent?.data?.toString() ?: "https://music.youtube.com/"
    runtime.webExtensionController
      .ensureBuiltIn(EXTENSION_URL, EXTENSION_ID)
      .accept({ extension ->
        Log.i(TAG, "extension installed: ${extension?.id}")
        if (extension == null) return@accept load()
        // The background script reports once the loader is registered
        extension.setMessageDelegate(messages, "pear")
        session.webExtensionController.setMessageDelegate(extension, messages, "pear")
        if (backgroundReady) load()
      }, { error ->
        Log.e(TAG, "installing the extension failed", error)
        load()
      })
  }

  private lateinit var start: String
  private var loaded = false

  /** Loads the page once, after the extension registered the loader. */
  private fun load() {
    if (loaded) return
    loaded = true
    session.loadUri(start)
  }

  private val messages = object : WebExtension.MessageDelegate {
    override fun onMessage(
      nativeApp: String,
      message: Any,
      sender: WebExtension.MessageSender,
    ): GeckoResult<Any>? {
      val json = message as? org.json.JSONObject
      when {
        json?.optBoolean("ready") == true -> {
          Log.i(TAG, "loader registered")
          backgroundReady = true
          load()
        }
        json?.has("error") == true -> {
          Log.e(TAG, "extension: ${json.optString("error")}")
          load()
        }
        else -> {
          // Too long for logcat; the latest one is kept in a file
          java.io.File(filesDir, "gecko-report.json").writeText(message.toString())
          Log.i(TAG, "report: ${message.toString().take(300)}")
        }
      }
      return null
    }
  }

  override fun onDestroy() {
    session.close()
    super.onDestroy()
  }

  companion object {
    private const val TAG = "PearGecko"
    private const val EXTENSION_ID = "pear-gecko@pear-devs.github.io"
    private const val EXTENSION_URL = "resource://android/assets/pear-gecko/"
    private var sharedRuntime: GeckoRuntime? = null
    /** Set once the extension's background script registered the loader. */
    @Volatile private var backgroundReady = false

    /** One GeckoRuntime per process, as GeckoView requires. */
    private fun runtime(activity: AppCompatActivity): GeckoRuntime =
      sharedRuntime ?: GeckoRuntime.create(
        activity.applicationContext,
        GeckoRuntimeSettings.Builder()
          .consoleOutput(true)
          .build(),
      ).also { sharedRuntime = it }
  }
}
