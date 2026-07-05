package com.sleeptracker.app

import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import android.util.Log

// android-activity（Slintのbackend-android-activity）はNativeActivityそのままでも動くが、
// nativeOnResume()のJNI呼び出しを行うためカスタムサブクラスにしている。
//
// 旧: 15分ごとの定期バックグラウンド送信（PeriodicWorkRequestBuilder）、および
// アプリを開くたびのDEVICE_ON即時送信（DriveSignalWorker）はどちらも廃止した。
// 「タブレットの電源が入っているか」しか分からず「実際に使っていたか」の証拠に
// ならないため、睡眠判定の材料としては信頼できないと判断した
// （scratchpad/sync_design_testでの検証・議論、および実際にDEVICE_ONが
// 睡眠セッションを誤って短く打ち切るバグを引き起こした件を参照）。
// 代わりにUsageReporterがUsageStatsManager由来の実際のアプリ利用区間を送信する。
class MainActivity : NativeActivity() {
    companion object {
        // NativeActivity自体はandroid.app.lib_name（sleep_tracker）をフレームワーク内部で
        // 直接dlopenするが、これはJavaのSystem.loadLibrary経由ではないため、ART側は
        // このクラスローダーに対して「ネイティブメソッドの探索先」として登録しない。
        // そのため下のnativeOnResume()のようなexternal fun宣言は、同じライブラリが
        // 既にプロセスにロード済みであってもUnsatisfiedLinkErrorになる。
        // 明示的にSystem.loadLibrary()を呼び直すことでART側にも登録させる
        // （同じライブラリの二重ロードは安全・無視される）。
        init {
            System.loadLibrary("sleep_tracker")
        }
    }

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

    // コールドスタート・タスク切り替えからの復帰・画面ロック解除からの復帰・
    // 権限設定画面からの帰還など、「人間がアプリの操作を再開した」場合は
    // 必ずAndroidがonResume()を呼ぶため、ここ一箇所で全経路をカバーする。
    override fun onResume() {
        super.onResume()
        val appCtx = applicationContext

        // 前回開いてから今回開くまでのタブレット利用区間をUsageStatsManagerから
        // 回収してDriveへ送信する（ネットワークI/Oを含むためバックグラウンドスレッドで）。
        Thread {
            UsageReporter.reportSinceLastCheck(appCtx)
        }.start()

        // Rust側の本同期（Drive/Sheetのpull+merge+push、同期アイコンの回転）を
        // キックする。画面OFFからの復帰時にRust側が反応せず同期アイコンも
        // 動かない、という指摘への対応（Rust側は5分ごとの定期タイマーしか
        // 持っておらず、onResume()相当のイベントを検知できていなかった）。
        nativeOnResume()
    }

    private external fun nativeOnResume()
}
