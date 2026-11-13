package com.madsbuch.learnpolish

import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

/**
 * Replaces the activity `tauri android init` generates (CI copies it over).
 *
 * The app draws edge to edge, so Android no longer shrinks it when the
 * keyboard opens; left alone it pans the whole window up instead, and the
 * header and the top of a chat slide off the screen. Here the keyboard's
 * height becomes bottom padding, so the web view ends just above the keyboard
 * and the page lays itself out in the space that is left. The navigation bar
 * part is left out: the page already keeps clear of it (safe-area insets).
 */
class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      val navigation = insets.getInsets(WindowInsetsCompat.Type.systemBars()).bottom
      view.setPadding(0, 0, 0, maxOf(0, keyboard - navigation))
      // The web view must not also make room for the keyboard.
      WindowInsetsCompat.Builder(insets)
        .setInsets(WindowInsetsCompat.Type.ime(), Insets.NONE)
        .build()
    }
  }
}
