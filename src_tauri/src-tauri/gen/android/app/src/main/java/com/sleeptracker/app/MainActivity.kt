package com.sleeptracker.app

import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  @Volatile private var currentTab: String = "home"
  private var appWebView: WebView? = null

  inner class AppBridge {
    @JavascriptInterface
    fun setTab(tab: String) {
      currentTab = tab
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    appWebView = webView
    webView.addJavascriptInterface(AppBridge(), "AppBridge")
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    WebView.setWebContentsDebuggingEnabled(true)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

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
  }
}
