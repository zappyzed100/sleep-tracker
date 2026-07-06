package com.sleeptracker.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.util.Log

// 画面OFF・非フォアグラウンド中もDrive同期を継続するための常駐フォアグラウンドサービス。
//
// 背景：android_bg.rsの5分タイマー（slint::Timer）はSlintのイベントループ上で動くため、
// アプリがフォアグラウンド（開いている間）しか動作しない。実機調査の結果、画面OFF中は
// このタイマーが一切発火せず、次にアプリを開く（onResume）までDrive同期が止まることが
// 判明した。
//
// 当初はここでHandler.postDelayed()による5分タイマーを実装したが、実機（HyperOS）で
// 検証したところ、フォアグラウンドサービスかつプロセスが生存中（他スレッドは動作継続）
// でも、Handlerタイマーだけが画面OFF中は一切発火しないことが判明した（OEM独自の
// スケジューラ抑制と見られる）。そのため周期実行はOSに正式スケジュールを委任する
// WorkManager（SyncWorker.kt、MainActivity.setupPeriodicSync参照）に任せ、
// このサービスは「常駐通知でプロセス優先度を保つ」「WifiLockでWi-Fi無線を維持する」
// 役割に専念する。
//
// Android 14 (API 34)以降、dataSync種別のフォアグラウンドサービスは24時間あたり合計
// 6時間までしか連続実行できない制限がある（onTimeout()参照）。就寝中の8時間程度は
// 超えうるため、タイムアウト時にサービスを一度停止して即座に再起動し、6時間カウンタを
// リセットすることで対応する。
class SyncService : Service() {
    companion object {
        private const val TAG = "SleepTracker"
        private const val CHANNEL_ID = "sync_service"
        private const val NOTIFICATION_ID = 1
    }

    private var wifiLock: WifiManager.WifiLock? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        acquireWifiLock()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = buildNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        Log.i(TAG, "[app] SyncService: started")
        return START_STICKY
    }

    // Android 14+: dataSync種別は24時間で合計6時間までの実行制限があり、超過すると
    // システムがこれを呼ぶ。ここでサービスを一度止めて即再起動することで6時間カウンタを
    // リセットする（呼び出し元のプロセス自体は生き続けるため、同期の連続性は保たれる）。
    override fun onTimeout(startId: Int, fgsType: Int) {
        Log.w(TAG, "[app] SyncService: onTimeout (6時間制限) — 再起動してカウンタをリセット")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf(startId)
        startService(Intent(this, SyncService::class.java))
    }

    override fun onDestroy() {
        wifiLock?.let { if (it.isHeld) it.release() }
        Log.i(TAG, "[app] SyncService: destroyed")
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    // 画面OFF中、Wi-Fi無線自体が省電力モードで切断されることが実機調査で確認できたため、
    // サービス生存中はWIFI_MODE_FULL_HIGH_PERFで維持する（バッテリー消費とのトレードオフ
    // だが、就寝中はどのみち充電中であることが多いため許容する）。
    private fun acquireWifiLock() {
        try {
            val wm = applicationContext.getSystemService(WIFI_SERVICE) as WifiManager
            val lock = wm.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "$packageName:sync")
            lock.setReferenceCounted(false)
            lock.acquire()
            wifiLock = lock
        } catch (e: Exception) {
            Log.w(TAG, "[app] SyncService: WifiLock ERROR ${e.message}")
        }
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.sync_service_channel_name),
            NotificationManager.IMPORTANCE_LOW
        )
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        return Notification.Builder(this, CHANNEL_ID)
            .setContentTitle(getString(R.string.sync_service_notification_title))
            .setContentText(getString(R.string.sync_service_notification_text))
            .setSmallIcon(R.mipmap.ic_launcher)
            .setOngoing(true)
            .build()
    }
}
