package com.sleeptracker.app

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.widget.FrameLayout
import android.widget.ImageView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

class MainActivity : TauriActivity() {
  companion object {
    // Shared across Activity instances so the new Activity after recreate() can read it.
    @Volatile var recreateInitiatedAt: Long = 0L
  }

  @Volatile private var currentTab: String = "home"
  private var appWebView: WebView? = null

  private var overlay: FrameLayout? = null
  private val uiHandler = Handler(Looper.getMainLooper())
  private var hideRunnable: Runnable? = null
  private var pauseTime: Long = Long.MAX_VALUE
  private var overlayShownAt: Long = 0L

  private fun dp(value: Int): Int =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value.toFloat(), resources.displayMetrics).toInt()

  inner class AppBridge {
    @JavascriptInterface
    fun setTab(tab: String) { currentTab = tab }

    // Called from JS when React has finished mounting.
    // First launch: waits until FIRST_LAUNCH_MIN_MS has elapsed before dismissing.
    // Deep sleep resume: dismisses immediately (postVisualStateCallback is primary signal).
    @JavascriptInterface
    fun notifyReady() {
      val elapsed = SystemClock.elapsedRealtime() - overlayShownAt
      val sinceRecreate = if (recreateInitiatedAt > 0L)
        SystemClock.elapsedRealtime() - recreateInitiatedAt else -1L
      Log.i("SleepTracker", "[overlay] notifyReady: elapsed=${elapsed}ms sinceRecreate=${sinceRecreate}ms")
      uiHandler.post {
        hideRunnable?.let { uiHandler.removeCallbacks(it) }
        overlay?.visibility = View.GONE
        Log.i("SleepTracker", "[overlay] hidden by notifyReady")
      }
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    webView.addJavascriptInterface(AppBridge(), "AppBridge")
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    // Dark window background so the gap between recreate() and overlay appearing isn't black.
    window.setBackgroundDrawable(android.graphics.drawable.ColorDrawable(0xFF1E1E2E.toInt()))
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Loading screen overlay
    val logo = ImageView(this).apply {
      setImageResource(R.mipmap.ic_launcher)
      scaleType = ImageView.ScaleType.FIT_CENTER
    }
    overlay = FrameLayout(this).apply {
      setBackgroundColor(0xFF1E1E2E.toInt())
      addView(logo, FrameLayout.LayoutParams(dp(160), dp(160), Gravity.CENTER))
      visibility = View.GONE
    }
    (window.decorView as ViewGroup).addView(
      overlay,
      ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
    )

    // Hardware back button
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() {
        if (currentTab == "settings") {
          appWebView?.evaluateJavascript(
            "window.dispatchEvent(new CustomEvent('__androidBack'))", null
          )
        } else {
          isEnabled = false
          this@MainActivity.onBackPressed()
          isEnabled = true
        }
      }
    })

    // Record startup time so DriveSignalWorker can distinguish startup run from background runs.
    applicationContext.getSharedPreferences("sleep_tracker", android.content.Context.MODE_PRIVATE)
      .edit().putLong("app_open_time_ms", System.currentTimeMillis()).apply()

    // Initialize WorkManager on a background thread — first call creates Room DB (disk I/O)
    // and blocks the main thread for several seconds if called here directly.
    val appCtx = applicationContext
    Thread {
      val workRequest = PeriodicWorkRequestBuilder<DriveSignalWorker>(15, TimeUnit.MINUTES)
        .setConstraints(
          Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()
        )
        .build()
      WorkManager.getInstance(appCtx).enqueueUniquePeriodicWork(
        "drive_signal",
        ExistingPeriodicWorkPolicy.KEEP,
        workRequest
      )
    }.start()
  }

  override fun onPause() {
    super.onPause()
    pauseTime = SystemClock.elapsedRealtime()
    Log.i("SleepTracker", "[lifecycle] onPause")
  }

  override fun onResume() {
    super.onResume()
    val isFirstLaunch = pauseTime == Long.MAX_VALUE
    val elapsed = if (isFirstLaunch) -1L else SystemClock.elapsedRealtime() - pauseTime
    Log.i("SleepTracker", "[lifecycle] onResume: isFirstLaunch=$isFirstLaunch elapsed=${elapsed}ms")
    if (!isFirstLaunch && elapsed >= 5 * 60 * 1000L) {
      // WebView takes 18-20s to unfreeze after long background. Cold start is ~500ms.
      recreateInitiatedAt = SystemClock.elapsedRealtime()
      Log.i("SleepTracker", "[lifecycle] long background → recreate() at ${recreateInitiatedAt}ms")
      recreate()
      return
    }
    // Log timing from recreate() for measurement (new Activity after recreate has isFirstLaunch=true)
    if (isFirstLaunch && recreateInitiatedAt > 0L) {
      val sinceRecreate = SystemClock.elapsedRealtime() - recreateInitiatedAt
      Log.i("SleepTracker", "[lifecycle] onResume after recreate: +${sinceRecreate}ms since recreate()")
    }
    if (isFirstLaunch || elapsed > 10_000L) showResumeOverlay(isFirstLaunch)
  }

  private fun showResumeOverlay(isFirstLaunch: Boolean = false) {
    val ov = overlay ?: return
    ov.visibility = View.VISIBLE
    overlayShownAt = SystemClock.elapsedRealtime()

    // After recreate() the new Activity looks like "first launch" but the WebView needs
    // a full cold start (~10s on this device). Keep overlay up until notifyReady() fires.
    val isFromRecreate = isFirstLaunch &&
        recreateInitiatedAt > 0L &&
        (overlayShownAt - recreateInitiatedAt) < 5_000L
    // Timeout: recreate cold-start → 30s (notifyReady() is primary, fires ~10s);
    //          normal first launch → 5s fallback; deep-sleep resume → 30s (postVisualStateCallback).
    val timeout = when {
      isFromRecreate -> 30_000L
      isFirstLaunch  ->  5_000L
      else           -> 30_000L
    }
    Log.i("SleepTracker", "[overlay] shown: isFirstLaunch=$isFirstLaunch isFromRecreate=$isFromRecreate timeout=${timeout}ms")

    hideRunnable?.let { uiHandler.removeCallbacks(it) }
    if (!isFirstLaunch) {
      // Deep sleep resume: hide when WebView repaints its first frame
      val wv = appWebView
      if (wv != null && android.os.Build.VERSION.SDK_INT >= 23) {
        wv.postVisualStateCallback(0L, object : WebView.VisualStateCallback() {
          override fun onComplete(requestId: Long) {
            val elapsed = SystemClock.elapsedRealtime() - overlayShownAt
            Log.i("SleepTracker", "[overlay] postVisualStateCallback.onComplete: elapsed=${elapsed}ms")
            runOnUiThread { ov.visibility = View.GONE }
          }
        })
      } else {
        Log.w("SleepTracker", "[overlay] postVisualStateCallback: skipped (wv=$wv, SDK=${android.os.Build.VERSION.SDK_INT})")
      }
    }
    val runnable = Runnable {
      val elapsed = SystemClock.elapsedRealtime() - overlayShownAt
      Log.i("SleepTracker", "[overlay] fallback timeout fired: elapsed=${elapsed}ms timeout=${timeout}ms")
      ov.visibility = View.GONE
    }
    hideRunnable = runnable
    uiHandler.postDelayed(runnable, timeout)
  }
}
