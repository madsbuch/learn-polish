package com.madsbuch.learnpolish

import android.content.res.Configuration
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Replaces the activity `tauri android init` generates (scripts/prepare-android.mjs
 * copies it over).
 *
 * The web view is laid out between the system bars, never under them: the
 * status bar, the navigation bar or taskbar, and a display cutout become
 * padding around it. The keyboard does too, so the page ends just above it
 * and lays itself out in the space that is left (left alone, Android would
 * pan the whole window up and slide the header off the screen). The web view
 * sees no insets, so the page's safe-area values are zero and nothing is
 * kept clear twice.
 *
 * Behind the bars is this view's background, which the page sets to its own
 * background colour (lib/theme.ts, through `AndroidSystemBars`), with dark or
 * light bar icons to match — so the bars look like part of the app in either
 * theme.
 */
class MainActivity : TauriActivity() {
  private lateinit var content: View

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // No grey scrim over a three-button navigation bar: its area shows the
    // app's own background.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      window.isNavigationBarContrastEnforced = false
      window.isStatusBarContrastEnforced = false
    }

    content = findViewById(android.R.id.content)
    // Until the page says otherwise, the colours of the theme it will start in.
    val night = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
    paintBars(if (night) DARK_BACKGROUND else LIGHT_BACKGROUND, night)

    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, keyboard))
      // Kept clear here, so the web view must not make room again.
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.addJavascriptInterface(SystemBars(), "AndroidSystemBars")
  }

  private fun paintBars(color: Int, dark: Boolean) {
    content.setBackgroundColor(color)
    WindowCompat.getInsetsController(window, window.decorView).apply {
      isAppearanceLightStatusBars = !dark
      isAppearanceLightNavigationBars = !dark
    }
  }

  /** What the page calls when its theme is applied or changes. */
  inner class SystemBars {
    @JavascriptInterface
    fun setColor(color: String, dark: Boolean) {
      val parsed = try {
        Color.parseColor(color)
      } catch (_: IllegalArgumentException) {
        return
      }
      runOnUiThread { paintBars(parsed, dark) }
    }
  }

  private companion object {
    // --bg in src/design/tokens.css.
    val LIGHT_BACKGROUND = Color.parseColor("#f6f5fb")
    val DARK_BACKGROUND = Color.parseColor("#0c0b16")
  }
}
