package io.github.astartes_gaming.emuera

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.os.Process

/**
 * Keeps a game alive while another app is in front: Android kills big background apps first,
 * and whatever wasn't saved goes with them. It lives in the game's process and ends with it.
 */
class GameService : Service() {
    override fun onBind(i: Intent?): IBinder? = null

    override fun onStartCommand(i: Intent?, flags: Int, startId: Int): Int {
        val builder = if (Build.VERSION.SDK_INT >= 26) {
            getSystemService(NotificationManager::class.java)
                .createNotificationChannel(NotificationChannel("game", "Running game", NotificationManager.IMPORTANCE_LOW))
            Notification.Builder(this, "game")
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        val back = Intent(this, GameActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val notification = builder
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(i?.getStringExtra("game") ?: "Emuera-Android")
            .setContentText("Game running. Tap to go back.")
            .setOngoing(true)
            .setContentIntent(PendingIntent.getActivity(this, 0, back, PendingIntent.FLAG_IMMUTABLE))
            .build()
        startForeground(1, notification)
        return START_NOT_STICKY
    }

    /** Swiping the game away in Recents ends it, as it did without a service. */
    override fun onTaskRemoved(i: Intent?) {
        gameMarker(this).delete()
        Process.killProcess(Process.myPid())
    }
}
