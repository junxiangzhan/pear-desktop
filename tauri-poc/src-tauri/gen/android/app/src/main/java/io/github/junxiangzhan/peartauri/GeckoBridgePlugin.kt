package io.github.junxiangzhan.peartauri

import android.app.Activity
import android.os.Handler
import android.os.Looper
import android.util.Log
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.WebExtension

@InvokeArg
class BridgeChannelArgs {
  lateinit var channel: Channel
}

@InvokeArg
class DeliverArgs {
  var port: String = GeckoBridge.PAGE
  var message: String? = null
  var eval: String? = null
}

/**
 * Commands for `src/gecko_bridge.rs`: the channel for messages from the
 * extension, and `deliver` for messages to it.
 */
@TauriPlugin
class GeckoBridgePlugin(activity: Activity) : Plugin(activity) {
  @Command
  fun setChannel(invoke: Invoke) {
    GeckoBridge.setChannel(invoke.parseArgs(BridgeChannelArgs::class.java).channel)
    invoke.resolve()
  }

  @Command
  fun pageUrl(invoke: Invoke) {
    invoke.resolve(JSObject().apply { put("url", GeckoBridge.pageUrl) })
  }

  @Command
  fun deliver(invoke: Invoke) {
    val args = invoke.parseArgs(DeliverArgs::class.java)
    GeckoBridge.deliver(args.port, args.message, args.eval)
    invoke.resolve()
  }
}

/**
 * Native messaging between the built-in extension (assets/pear-gecko) and
 * Rust. Each extension context opens a port named "pear": the content script
 * in the page (`page`) and the background script (`background`). Messages are
 * JSON strings, passed through as `{ m }` so they are parsed only at the ends.
 */
object GeckoBridge {
  const val PAGE = "page"
  const val BACKGROUND = "background"
  private const val TAG = "PearGeckoBridge"

  private val main = Handler(Looper.getMainLooper())
  /** What GeckoPage shows, for the menu's Copy current URL. */
  @Volatile var pageUrl: String? = null
  private val ports = HashMap<String, WebExtension.Port>()
  private var channel: Channel? = null
  /** Messages that arrived before Rust registered its channel. */
  private val early = ArrayList<Pair<String, String>>()

  fun setChannel(channel: Channel) = main.post {
    this.channel = channel
    early.forEach { (port, message) -> toRust(port, message) }
    early.clear()
  }

  /**
   * The delegate for `connectNative("pear")` from the given context;
   * one-off `sendNativeMessage`s go to `onMessage`.
   */
  fun delegate(name: String, onMessage: (JSONObject) -> Unit) = object : WebExtension.MessageDelegate {
    override fun onMessage(
      nativeApp: String,
      message: Any,
      sender: WebExtension.MessageSender,
    ): GeckoResult<Any>? {
      (message as? JSONObject)?.let(onMessage)
      return null
    }

    override fun onConnect(port: WebExtension.Port) {
      Log.i(TAG, "$name connected")
      ports[name] = port
      port.setDelegate(object : WebExtension.PortDelegate {
        override fun onPortMessage(message: Any, port: WebExtension.Port) {
          (message as? JSONObject)?.optString("m")?.let { toRust(name, it) }
        }

        override fun onDisconnect(port: WebExtension.Port) {
          if (ports[name] === port) ports.remove(name)
        }
      })
    }
  }

  // On the UI thread, like the port callbacks; Rust runs sync commands here
  private fun toRust(port: String, message: String) {
    val channel = channel ?: return run { early.add(port to message) }
    channel.send(JSObject().apply {
      put("port", port)
      put("message", message)
    })
  }

  fun deliver(port: String, message: String?, eval: String?) = main.post {
    val target = ports[port] ?: return@post run { Log.w(TAG, "no $port port for a message") }
    target.postMessage(JSONObject().apply {
      if (message != null) put("m", message)
      if (eval != null) put("eval", eval)
    })
  }
}
