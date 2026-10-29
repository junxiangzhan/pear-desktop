package io.github.junxiangzhan.peartauri

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import java.net.URL
import kotlin.concurrent.thread

/** What the notification and the media session show; sent by Rust. */
data class PlaybackInfo(
  val title: String,
  val artist: String,
  val album: String?,
  val artworkUrl: String?,
  val isPaused: Boolean,
  val durationMs: Long,
  val positionMs: Long,
)

/**
 * Keeps playback going with the app in the background: a `mediaPlayback`
 * foreground service stops Android from freezing the process (the WebView
 * plays the audio), and its media session gives the system media controls.
 * Control requests go back to Rust through [actions].
 */
class PlaybackService : Service() {
  companion object {
    private const val CHANNEL_ID = "playback"
    private const val NOTIFICATION_ID = 1
    private const val ACTION = "io.github.junxiangzhan.peartauri.PLAYBACK_ACTION"

    /** Sends a control request (`play`, `pause`, `next`, `previous`, `seek`) to Rust. */
    @Volatile var actions: ((action: String, value: Long) -> Unit)? = null

    @Volatile private var info: PlaybackInfo? = null
    @Volatile private var instance: PlaybackService? = null

    /** Whether a media session is up (MainActivity keeps the WebView active then). */
    val isRunning: Boolean get() = instance != null

    fun update(context: Context, playback: PlaybackInfo) {
      info = playback
      val running = instance
      if (running != null) {
        running.handler.post { running.refresh() }
      } else {
        val intent = Intent(context, PlaybackService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
          context.startForegroundService(intent)
        } else {
          context.startService(intent)
        }
      }
    }

    fun stop(context: Context) {
      info = null
      context.stopService(Intent(context, PlaybackService::class.java))
    }
  }

  private val handler = Handler(Looper.getMainLooper())
  private lateinit var session: MediaSession
  private var artworkUrl: String? = null
  private var artwork: Bitmap? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    instance = this
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL_ID, "Playback", NotificationManager.IMPORTANCE_LOW)
      )
    }
    session = MediaSession(this, "PearDesktop").apply {
      setCallback(object : MediaSession.Callback() {
        override fun onPlay() = send("play")
        override fun onPause() = send("pause")
        override fun onSkipToNext() = send("next")
        override fun onSkipToPrevious() = send("previous")
        override fun onSeekTo(pos: Long) = send("seek", pos)
        override fun onStop() = send("pause")
      })
      setSessionActivity(openAppIntent())
      isActive = true
    }
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    // Buttons of the notification (before Android 13 the system draws these)
    when (intent?.getStringExtra(ACTION)) {
      "toggle" -> send(if (info?.isPaused == false) "pause" else "play")
      "next" -> send("next")
      "previous" -> send("previous")
    }
    refresh()
    return START_NOT_STICKY
  }

  /** The WebView goes away with the task, so playback cannot outlive it. */
  override fun onTaskRemoved(rootIntent: Intent?) {
    stopSelf()
  }

  override fun onDestroy() {
    instance = null
    session.isActive = false
    session.release()
    super.onDestroy()
  }

  private fun send(action: String, value: Long = 0) {
    actions?.invoke(action, value)
  }

  private fun refresh() {
    val playback = info ?: return stopSelf()
    if (playback.artworkUrl != artworkUrl) loadArtwork(playback.artworkUrl)

    session.setMetadata(
      MediaMetadata.Builder()
        .putString(MediaMetadata.METADATA_KEY_TITLE, playback.title)
        .putString(MediaMetadata.METADATA_KEY_ARTIST, playback.artist)
        .putString(MediaMetadata.METADATA_KEY_ALBUM, playback.album)
        .putLong(MediaMetadata.METADATA_KEY_DURATION, playback.durationMs)
        .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, artwork)
        .build()
    )
    session.setPlaybackState(
      PlaybackState.Builder()
        .setActions(
          PlaybackState.ACTION_PLAY or PlaybackState.ACTION_PAUSE or
            PlaybackState.ACTION_PLAY_PAUSE or PlaybackState.ACTION_SKIP_TO_NEXT or
            PlaybackState.ACTION_SKIP_TO_PREVIOUS or PlaybackState.ACTION_SEEK_TO
        )
        .setState(
          if (playback.isPaused) PlaybackState.STATE_PAUSED else PlaybackState.STATE_PLAYING,
          playback.positionMs,
          if (playback.isPaused) 0f else 1f,
          SystemClock.elapsedRealtime(),
        )
        .build()
    )

    val notification = buildNotification(playback)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
  }

  private fun buildNotification(playback: PlaybackInfo): Notification {
    val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      Notification.Builder(this, CHANNEL_ID)
    } else {
      @Suppress("DEPRECATION") Notification.Builder(this)
    }
    val playPause = if (playback.isPaused) {
      Notification.Action.Builder(null, "Play", actionIntent("toggle", 1)).build()
    } else {
      Notification.Action.Builder(null, "Pause", actionIntent("toggle", 1)).build()
    }
    return builder
      .setSmallIcon(if (playback.isPaused) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play)
      .setContentTitle(playback.title)
      .setContentText(playback.artist)
      .setLargeIcon(artwork)
      .setContentIntent(openAppIntent())
      .setOngoing(true)
      .setVisibility(Notification.VISIBILITY_PUBLIC)
      .addAction(Notification.Action.Builder(null, "Previous", actionIntent("previous", 0)).build())
      .addAction(playPause)
      .addAction(Notification.Action.Builder(null, "Next", actionIntent("next", 2)).build())
      .setStyle(Notification.MediaStyle().setMediaSession(session.sessionToken).setShowActionsInCompactView(0, 1, 2))
      .build()
  }

  private fun actionIntent(action: String, requestCode: Int): PendingIntent =
    PendingIntent.getService(
      this,
      requestCode,
      Intent(this, PlaybackService::class.java).putExtra(ACTION, action),
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

  private fun openAppIntent(): PendingIntent =
    PendingIntent.getActivity(
      this,
      0,
      Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

  private fun loadArtwork(url: String?) {
    artworkUrl = url
    artwork = null
    if (url.isNullOrEmpty()) return
    thread(name = "artwork") {
      val bitmap = runCatching { URL(url).openStream().use(BitmapFactory::decodeStream) }.getOrNull()
      handler.post {
        if (artworkUrl == url) {
          artwork = bitmap
          refresh()
        }
      }
    }
  }
}
