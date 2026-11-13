package com.polski.songplayer

import androidx.core.content.FileProvider

/** Our own FileProvider class, so its manifest entry never merges with another library's. */
class ShareFileProvider : FileProvider()
