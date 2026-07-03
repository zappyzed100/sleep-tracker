package com.sleeptracker.app

import android.app.NativeActivity
import android.os.Bundle
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
