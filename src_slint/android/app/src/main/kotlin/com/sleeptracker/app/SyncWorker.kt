package com.sleeptracker.app

import android.content.Context
import android.util.Log
import androidx.work.Worker
import androidx.work.WorkerParameters

// WorkManager経由で15分おきに（OSが正式にスケジュールを保証する形で）Drive同期を
// キックするWorker。SyncService.ktのHandlerタイマーが画面OFF中に発火しないことが
// 実機検証で判明したため、代わりにこちらがOSに周期実行を委任する（MainActivity.
// setupPeriodicSync参照）。
//
// doWork()はWorkManagerのバックグラウンドスレッドプールで呼ばれるが、実際の同期処理
// （Rust側run_sync）は非同期にスレッドを立てて即returnする作りのため、ここでは
// キックするだけで完了を待たずにResult.success()を返す。
class SyncWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    companion object {
        private const val TAG = "SleepTracker"

        init {
            System.loadLibrary("sleep_tracker")
        }
    }

    override fun doWork(): Result {
        Log.i(TAG, "[app] SyncWorker: doWork")
        nativeBackgroundSync()
        return Result.success()
    }

    // Rust側（platform/android_bg.rs）のJNIエントリポイント。既存のnativeOnResumeと
    // 同じ同期処理（run_sync）を再利用する。
    private external fun nativeBackgroundSync()
}
