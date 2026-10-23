package com.github.pear_devs.tauri_poc

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.util.Log
import android.view.View
import android.webkit.WebView
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.UserAgentMetadata
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    // YouTube Music is dark: light system bar icons on its background
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
    )
    super.onCreate(savedInstanceState)
    // Edge-to-edge is enforced from Android 15; keep the page out of the
    // status bar, the navigation bar and display cutouts
    val content = findViewById<View>(android.R.id.content)
    content.setBackgroundColor(YTM_BACKGROUND)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    presentAsDesktopChrome(webView)
    // Opened from a YouTube Music link: load it after wry's start URL. wry
    // posts that load right after this returns, so post behind it
    linkUrl(intent)?.let { url -> webView.post { webView.post { webView.loadUrl(url) } } }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    linkUrl(intent)?.let { url -> webView?.loadUrl(url) }
  }

  /** The music.youtube.com link this activity was opened with, if any. */
  private fun linkUrl(intent: Intent?): String? {
    val uri = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.data ?: return null
    return uri.toString().takeIf { uri.scheme == "https" && uri.host == "music.youtube.com" }
  }

  /**
   * The renderer plugins need the desktop site, and Google sign-in refuses
   * embedded WebViews ("This browser or app may not be secure"). Present the
   * WebView as desktop Chrome of the same version, consistently: the UA
   * string, the client hints (Sec-CH-UA / navigator.userAgentData, which
   * otherwise list "Android WebView"), and no X-Requested-With header (which
   * otherwise carries this package name). Runs after wry set its UA and
   * before the first page load.
   */
  private fun presentAsDesktopChrome(webView: WebView) {
    val fullVersion = WebViewCompat.getCurrentWebViewPackage(this)?.versionName ?: "141.0.0.0"
    val major = fullVersion.substringBefore('.')
    webView.settings.userAgentString =
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/$major.0.0.0 Safari/537.36"

    if (WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA)) {
      val brand = { name: String, version: String, full: String ->
        UserAgentMetadata.BrandVersion.Builder()
          .setBrand(name)
          .setMajorVersion(version)
          .setFullVersion(full)
          .build()
      }
      WebSettingsCompat.setUserAgentMetadata(
        webView.settings,
        UserAgentMetadata.Builder()
          .setBrandVersionList(
            listOf(
              brand("Google Chrome", major, fullVersion),
              brand("Chromium", major, fullVersion),
              brand("Not_A Brand", "24", "24.0.0.0"),
            )
          )
          .setFullVersion(fullVersion)
          .setPlatform("Linux")
          .setPlatformVersion("")
          .setArchitecture("x86")
          .setBitness(64)
          .setModel("")
          .setMobile(false)
          .build(),
      )
    }
    if (WebViewFeature.isFeatureSupported(WebViewFeature.REQUESTED_WITH_HEADER_ALLOW_LIST)) {
      WebSettingsCompat.setRequestedWithHeaderOriginAllowList(webView.settings, emptySet())
    }
    Log.i(
      TAG,
      "WebView $fullVersion as desktop Chrome: client hints " +
        WebViewFeature.isFeatureSupported(WebViewFeature.USER_AGENT_METADATA) +
        ", X-Requested-With allow-list " +
        WebViewFeature.isFeatureSupported(WebViewFeature.REQUESTED_WITH_HEADER_ALLOW_LIST),
    )
  }

  // Chromium pauses media with a video track once the WebView is paused or
  // its window hidden (YTM always plays through <video>). While
  // PlaybackService holds the session, keep the WebView looking visible.
  override fun onPause() {
    super.onPause()
    if (PlaybackService.isRunning) webView?.onResume()
  }

  override fun onStop() {
    super.onStop()
    if (!PlaybackService.isRunning) return
    // The window turns invisible after onStop; report it visible again then
    webView?.postDelayed({
      if (PlaybackService.isRunning) webView?.dispatchWindowVisibilityChanged(View.VISIBLE)
    }, 300)
  }

  private companion object {
    const val TAG = "PearMainActivity"

    // YouTube Music's page background (#030303)
    val YTM_BACKGROUND = Color.rgb(3, 3, 3)
  }
}
