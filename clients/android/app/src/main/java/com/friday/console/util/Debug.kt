package com.friday.console.util

import android.content.Context
import java.io.File

/**
 * Fallback logger: logcat on this device is not surfacing app tags, so we also
 * append to a file in the app's internal cache dir. Pull with:
 *   adb shell su -c "cat /data/data/com.friday.console/cache/ws.log"
 */
object Debug {
    lateinit var ctx: Context

    fun init(context: Context) {
        ctx = context.applicationContext
    }

    fun log(tag: String, msg: String) {
        try {
            val f = File(ctx.cacheDir, "ws.log")
            f.appendText("${System.currentTimeMillis()} $tag: $msg\n")
        } catch (_: Exception) { }
    }
}
