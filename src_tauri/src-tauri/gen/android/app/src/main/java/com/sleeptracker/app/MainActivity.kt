package com.sleeptracker.app

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
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
  @Volatile private var currentTab: String = "home"
  private var appWebView: WebView? = null

  private var overlay: FrameLayout? = null
  private val uiHandler = Handler(Looper.getMainLooper())
  private var hideRunnable: Runnable? = null
  private var pauseTime: Long = Long.MAX_VALUE

  private fun dp(value: Int): Int =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value.toFloat(), resources.displayMetrics).toInt()

  inner class AppBridge {
    @JavascriptInterface
    fun setTab(tab: String) { currentTab = tab }
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    webView.addJavascriptInterface(AppBridge(), "AppBridge")
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    WebView.setWebContentsDebuggingEnabled(true)
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
  }

  override fun onResume() {
    super.onResume()
    val elapsed = SystemClock.elapsedRealtime() - pauseTime
    if (elapsed > 10_000L) showResumeOverlay()
  }

  private fun showResumeOverlay() {
    val ov = overlay ?: return
    ov.visibility = View.VISIBLE
    hideRunnable?.let { uiHandler.removeCallbacks(it) }
    val wv = appWebView
    if (wv != null && android.os.Build.VERSION.SDK_INT >= 23) {
      wv.postVisualStateCallback(0L, object : WebView.VisualStateCallback() {
        override fun onComplete(requestId: Long) {
          runOnUiThread { ov.visibility = View.GONE }
        }
      })
    }
    val runnable = Runnable { ov.visibility = View.GONE }
    hideRunnable = runnable
    uiHandler.postDelayed(runnable, 2000L)
  }
}
