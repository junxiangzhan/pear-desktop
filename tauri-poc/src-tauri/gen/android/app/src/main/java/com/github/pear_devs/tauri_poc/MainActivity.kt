package com.github.pear_devs.tauri_poc

import android.os.Bundle
import android.view.View
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  private var webView: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
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
}
