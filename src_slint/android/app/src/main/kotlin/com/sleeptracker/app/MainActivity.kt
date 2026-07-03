package com.sleeptracker.app

import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

// android-activity（Slintのbackend-android-activity）はNativeActivityそのままでも動くが、
// WorkManagerによる真のバックグラウンド同期（DriveSignalWorker）を登録するために
// カスタムサブクラスにしている。Tauri版 MainActivity.kt のWorkManager登録部分を移植。
class MainActivity : NativeActivity() {
    private val networkConstraints = Constraints.Builder()
        .setRequiredNetworkType(NetworkType.CONNECTED)
        .build()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        requestIgnoreBatteryOptimizations()

        // WorkManagerの初期化はディスクI/O（Room DB作成）を伴いメインスレッドを
        // 数秒ブロックすることがあるためバックグラウンドスレッドで行う。
        val appCtx = applicationContext
        Thread {
            val workRequest = PeriodicWorkRequestBuilder<DriveSignalWorker>(15, TimeUnit.MINUTES)
                .setConstraints(networkConstraints)
                .build()
            WorkManager.getInstance(appCtx).enqueueUniquePeriodicWork(
                "drive_signal",
                ExistingPeriodicWorkPolicy.KEEP,
                workRequest
            )
        }.start()
    }

    // Dozeモード（画面OFF・静止状態が続くと入る省電力モード）に入ると、WorkManagerの
    // 15分間隔は「最短でもこの間隔」に格下げされ、実行が数十分〜数時間単位で遅延する。
    // スリープ中（＝画面OFFが最も長く続く場面）ほど遅延しやすいため、バッテリー最適化の
    // 除外をユーザーに求めてDozeの影響を軽減する。既に許可済みなら何もしない。
    private fun requestIgnoreBatteryOptimizations() {
        val pm = getSystemService(POWER_SERVICE) as PowerManager
        if (pm.isIgnoringBatteryOptimizations(packageName)) return
        try {
            startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
                data = Uri.parse("package:$packageName")
            })
        } catch (e: Exception) {
            // 一部端末（MIUI等）でこのIntentに対応するActivityが存在しない場合がある。
            // その場合は端末の設定アプリから手動で許可してもらう必要がある。
            Log.w("SleepTracker", "[app] requestIgnoreBatteryOptimizations: ERROR ${e.message}")
        }
    }

    // 起動時／バックグラウンドから画面ONで復帰時／スリープから画面ONで復帰時、
    // いずれもAndroidはonResume()を呼ぶため、ここ一箇所でDEVICE_ONの即時送信をカバーする。
    override fun onResume() {
        super.onResume()
        val appCtx = applicationContext
        Thread {
            val request = OneTimeWorkRequestBuilder<DriveSignalWorker>()
                .setConstraints(networkConstraints)
                .build()
            WorkManager.getInstance(appCtx).enqueueUniqueWork(
                "drive_signal_immediate",
                ExistingWorkPolicy.REPLACE,
                request
            )
        }.start()
    }
}
