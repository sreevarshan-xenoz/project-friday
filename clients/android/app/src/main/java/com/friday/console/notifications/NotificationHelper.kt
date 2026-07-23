package com.friday.console.notifications

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import com.friday.console.ui.LockScreenActivity

/**
 * M6.2.6 — notification actions. A failed task surfaces a notification with
 * [Open] / [Retry] / [Ignore] buttons that emit real protocol commands via
 * [CommandActionReceiver] (no AI logic on the phone).
 */
object NotificationHelper {
    private const val CHANNEL = "friday_remote"

    fun ensureChannel(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val mgr = context.getSystemService(NotificationManager::class.java)
        if (mgr.getNotificationChannel(CHANNEL) == null) {
            val channel = NotificationChannel(
                CHANNEL,
                "FRIDAY Remote",
                NotificationManager.IMPORTANCE_HIGH,
            )
            mgr.createNotificationChannel(channel)
        }
    }

    fun showTaskFailed(context: Context, task: String) {
        ensureChannel(context)
        val open = PendingIntent.getActivity(
            context,
            0,
            Intent(context, LockScreenActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val note = NotificationCompat.Builder(context, CHANNEL)
            .setContentTitle("FRIDAY")
            .setContentText("$task failed.")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(open)
            .addAction(0, "Open", open)
            .addAction(0, "Retry", action(context, "retry", task))
            .addAction(0, "Ignore", action(context, "ignore", task))
            .setAutoCancel(true)
            .build()
        context.getSystemService(NotificationManager::class.java).notify(task.hashCode(), note)
    }

    private fun action(context: Context, kind: String, task: String): PendingIntent {
        val intent = Intent(context, CommandActionReceiver::class.java).apply {
            putExtra("kind", kind)
            putExtra("task", task)
        }
        return PendingIntent.getBroadcast(
            context,
            kind.hashCode(),
            intent,
            PendingIntent.FLAG_IMMUTABLE,
        )
    }
}
