package com.sleeptracker.app

import android.app.NativeActivity
import android.content.Context
import android.os.Bundle
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

// android-activity（Slintのbackend-android-activity）はNativeActivityそのままでも動くが、
// WorkManagerによる真のバックグラウンド同期（DriveSignalWorker）を登録するために
// カスタムサブクラスにしている。Tauri版 MainActivity.kt のWorkManager登録部分を移植。
class MainActivity : NativeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // 起動時刻を記録（DriveSignalWorkerが起動直後の実行かどうか判定するため）
        applicationContext.getSharedPreferences("sleep_tracker", Context.MODE_PRIVATE)
            .edit().putLong("app_open_time_ms", System.currentTimeMillis()).apply()

        // WorkManagerの初期化はディスクI/O（Room DB作成）を伴いメインスレッドを
        // 数秒ブロックすることがあるためバックグラウンドスレッドで行う。
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
}
