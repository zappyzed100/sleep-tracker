package com.sleeptracker.app

import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.util.Log
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager

// android-activity（Slintのbackend-android-activity）はNativeActivityそのままでも動くが、
// WorkManagerによるDEVICE_ON即時送信（DriveSignalWorker）を登録するためにカスタム
// サブクラスにしている。
//
// 旧: 15分ごとの定期バックグラウンド送信（PeriodicWorkRequestBuilder）は廃止した。
// 「タブレットの電源が入っているか」しか分からず「実際に使っていたか」の証拠に
// ならないため、睡眠判定の材料としては信頼できないと判断した
// （scratchpad/sync_design_testでの検証・議論を参照）。代わりにUsageReporterが
// UsageStatsManager由来の実際のアプリ利用区間を送信する。
class MainActivity : NativeActivity() {
    private val networkConstraints = Constraints.Builder()
        .setRequiredNetworkType(NetworkType.CONNECTED)
        .build()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        requestIgnoreBatteryOptimizations()
        UsageReporter.requestUsageAccess(this)
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

        // 前回開いてから今回開くまでのタブレット利用区間をUsageStatsManagerから
        // 回収してDriveへ送信する（ネットワークI/Oを含むためバックグラウンドスレッドで）。
        Thread {
            UsageReporter.reportSinceLastCheck(appCtx)
        }.start()
    }
}
