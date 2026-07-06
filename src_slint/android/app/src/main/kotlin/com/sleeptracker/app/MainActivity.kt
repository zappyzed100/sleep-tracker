package com.sleeptracker.app

import android.app.Activity
import android.app.NativeActivity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.DocumentsContract
import android.provider.Settings
import android.util.Log
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.io.File
import java.util.concurrent.TimeUnit

// android-activity（Slintのbackend-android-activity）はNativeActivityそのままでも動くが、
// nativeOnResume()のJNI呼び出しを行うためカスタムサブクラスにしている。
//
// 旧: アプリを開くたびのDEVICE_ON即時送信（DriveSignalWorker）は廃止した。
// 「タブレットの電源が入っているか」しか分からず「実際に使っていたか」の証拠に
// ならないため、睡眠判定の材料としては信頼できないと判断した
// （scratchpad/sync_design_testでの検証・議論、および実際にDEVICE_ONが
// 睡眠セッションを誤って短く打ち切るバグを引き起こした件を参照）。
// 代わりにUsageReporterがUsageStatsManager由来の実際のアプリ利用区間を送信する。
//
// 15分ごとの定期バックグラウンド送信（PeriodicWorkRequestBuilder）は上記の理由で
// 一度廃止したが、今回はDEVICE_ON送信としてではなく「Drive同期をキックするだけ」の
// 目的でsetupPeriodicSync()として復活させている（睡眠判定用のイベントは書き込まない）。
// SyncService.ktのHandlerタイマーが画面OFF中に発火しないことが実機検証で判明した
// ため、OSに正式スケジュールを委任するWorkManagerに置き換えた。
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

        private const val REQUEST_RESTORE_FILE = 1001
        private const val REQUEST_NOTIFICATION_PERMISSION = 1002
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        requestIgnoreBatteryOptimizations()
        requestNotificationPermission()
        UsageReporter.requestUsageAccess(this)
        startSyncService()
        setupPeriodicSync()
    }

    // WorkManagerに15分間隔でのDrive同期を委任する（SyncWorker.kt参照）。
    // PeriodicWorkRequestの最短間隔は15分固定（OS側の制約）。ExistingPeriodicWorkPolicy.KEEP
    // により、アプリ再起動のたびに呼んでも既存のスケジュールが重複登録されない。
    private fun setupPeriodicSync() {
        val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES).build()
        WorkManager.getInstance(this).enqueueUniquePeriodicWork(
            "drive_sync",
            ExistingPeriodicWorkPolicy.KEEP,
            request
        )
    }

    // 画面OFF中も動き続けるSyncService（常駐フォアグラウンドサービス）を起動する。
    // 既に起動中でも同じIntentでの再startServiceは安全（onStartCommandが再実行されるだけ）。
    private fun startSyncService() {
        val intent = Intent(this, SyncService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    // Android 13+では通知の表示にランタイム権限が必要。拒否されてもSyncService自体は
    // 動作する（通知が非表示になるだけ）ため、拒否時の特別なハンドリングは不要。
    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        if (ContextCompat.checkSelfPermission(this, android.Manifest.permission.POST_NOTIFICATIONS)
            == PackageManager.PERMISSION_GRANTED) return
        ActivityCompat.requestPermissions(
            this,
            arrayOf(android.Manifest.permission.POST_NOTIFICATIONS),
            REQUEST_NOTIFICATION_PERMISSION
        )
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

    // Rust側（platform/android_restore.rs）からJNI経由で呼ばれる。「復元」ボタンの
    // 2回目クリックでシステムのファイルピッカー(ACTION_OPEN_DOCUMENT)を起動する。
    // rfd（Windowsのみ対応）の代わりにこちらを使う。
    fun launchRestorePicker() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
            // ベストエフォート: 自動/手動バックアップの保存先（このアプリ専用の外部ストレージ
            // 領域のbackups/）を初期表示フォルダにしようとする。Android 11以降はSAFの
            // ファイルピッカーがAndroid/data配下へのナビゲーションをOS側で制限しているため、
            // 端末・ファイルマネージャー実装によっては効かないことがある（その場合は
            // ピッカーの既定の場所が開くだけで、ユーザーが手動で選び直せる）。
            val backupsDir = File(getExternalFilesDir(null), "backups")
            if (backupsDir.exists()) {
                putExtra(DocumentsContract.EXTRA_INITIAL_URI, Uri.fromFile(backupsDir))
            }
        }
        try {
            startActivityForResult(intent, REQUEST_RESTORE_FILE)
        } catch (e: Exception) {
            Log.w("SleepTracker", "[app] launchRestorePicker: ERROR ${e.message}")
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_RESTORE_FILE) return

        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) {
            nativeRestorePicked(null)
            return
        }
        try {
            val content = contentResolver.openInputStream(uri)?.use { input ->
                input.readBytes().toString(Charsets.UTF_8)
            }
            nativeRestorePicked(content)
        } catch (e: Exception) {
            Log.w("SleepTracker", "[app] restore file read: ERROR ${e.message}")
            nativeRestorePicked(null)
        }
    }

    private external fun nativeRestorePicked(content: String?)
}
