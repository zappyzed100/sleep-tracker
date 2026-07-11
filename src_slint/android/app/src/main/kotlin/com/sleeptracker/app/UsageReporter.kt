package com.sleeptracker.app

import android.app.AppOpsManager
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.Intent
import android.os.Process
import android.provider.Settings
import android.util.Log
import java.net.HttpURLConnection
import java.net.URL

// タブレットの画面が実際にON（インタラクティブ）だった区間をUsageStatsManagerから
// 検出し、SCREEN_ON_START/SCREEN_ON_ENDとしてDriveへ送信する。
//
// 背景：以前はMOVE_TO_FOREGROUND/MOVE_TO_BACKGROUND（どのアプリがアクティブか）を
// 使っていたが、画面が自動消灯した後もOS・機種（MIUI等）によっては直前のアプリが
// 「まだフォアグラウンド扱い」のまま残り続け、次にアプリを切り替えるまで
// MOVE_TO_BACKGROUNDが発生しないバグがあった。その結果、画面OFFで何時間も
// 放置していた時間まで丸ごと利用区間として記録されてしまっていた。
// SCREEN_INTERACTIVE/SCREEN_NON_INTERACTIVE（画面の実ON/OFFイベント、API 28+）を
// 使えば、タブレットを実際に触っていた時間だけを正確に切り出せる。
// アプリ単位の識別（どのアプリを使ったか）はもう行わない。
//
// フィルタ・統合（短すぎる点灯の除外、細切れの統合）はRust側(events.rs)の
// coalesce_and_filter_screen_onが一括で担うため、ここでは検出した区間を
// そのまま全部送るだけでよい（Kotlin側にしきい値を重複させない）。
object UsageReporter {
    private const val TAG = "SleepTracker"
    private const val PREFS = "sleep_tracker"
    private const val KEY_LAST_QUERY_MS = "last_usage_query_ms"
    // UsageStatsManagerの履歴保持期間は機種依存だが、遡りすぎて古いデータを
    // 大量送信しないための安全策として上限を設ける。
    private const val MAX_LOOKBACK_MS = 7L * 24 * 60 * 60 * 1000

    fun hasUsageAccess(context: Context): Boolean {
        val appOps = context.getSystemService(Context.APP_OPS_SERVICE) as AppOpsManager
        val mode = appOps.checkOpNoThrow(
            AppOpsManager.OPSTR_GET_USAGE_STATS,
            Process.myUid(),
            context.packageName
        )
        return mode == AppOpsManager.MODE_ALLOWED
    }

    fun requestUsageAccess(context: Context) {
        if (hasUsageAccess(context)) return
        try {
            context.startActivity(Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK
            })
        } catch (e: Exception) {
            Log.w(TAG, "[app] requestUsageAccess: ERROR ${e.message}")
        }
    }

    // 前回確認時刻〜現在までの画面ON区間を集めてDriveへ送信する。
    // 呼び出し側でバックグラウンドスレッドから呼ぶこと（ネットワークI/Oを含む）。
    fun reportSinceLastCheck(context: Context) {
        if (!hasUsageAccess(context)) {
            Log.w(TAG, "[app] UsageReporter: usage access not granted — skip")
            return
        }
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val now = System.currentTimeMillis()
        val lastQuery = prefs.getLong(KEY_LAST_QUERY_MS, 0L)
        // 初回（チェックポイントが無い）は直近1時間だけ遡る（大量の古いデータを
        // いきなり送りつけないため）。2回目以降は前回確認時刻から。
        val from = if (lastQuery > 0L) maxOf(lastQuery, now - MAX_LOOKBACK_MS) else now - 60 * 60 * 1000

        val intervals = queryScreenOnIntervals(context, from, now)
        if (intervals.isEmpty()) {
            prefs.edit().putLong(KEY_LAST_QUERY_MS, now).apply()
            Log.i(TAG, "[app] UsageReporter: no screen-on intervals found")
            return
        }

        val config = readConfig(context)
        val baseUrl = config?.optString("mobile_url")?.trim().orEmpty()
        val secret  = config?.optString("mobile_secret")?.trim().orEmpty()
        if (baseUrl.isEmpty() || secret.isEmpty()) {
            Log.w(TAG, "[app] UsageReporter: mobile_url/secret未設定 — skip send")
            prefs.edit().putLong(KEY_LAST_QUERY_MS, now).apply()
            return
        }

        var allOk = true
        for ((s, e) in intervals) {
            val okStart = post(baseUrl, secret, "SCREEN_ON_START", s)
            val okEnd   = post(baseUrl, secret, "SCREEN_ON_END", e)
            if (!okStart || !okEnd) allOk = false
        }
        Log.i(TAG, "[app] UsageReporter: sent ${intervals.size} interval(s), allOk=$allOk")
        prefs.edit().putLong(KEY_LAST_QUERY_MS, now).apply()
    }

    // UsageEvents から画面がインタラクティブ（ON）だった区間を再構成する。
    // SCREEN_NON_INTERACTIVEが来ないまま問い合わせ時刻に達した場合は、その時点で
    // 画面がONのままとみなして now で区切る。
    private fun queryScreenOnIntervals(context: Context, from: Long, to: Long): List<Pair<Long, Long>> {
        val usm = context.getSystemService(Context.USAGE_STATS_SERVICE) as UsageStatsManager
        val events = usm.queryEvents(from, to)
        val intervals = mutableListOf<Pair<Long, Long>>()
        var pendingStart: Long? = null
        val event = UsageEvents.Event()
        while (events.hasNextEvent()) {
            events.getNextEvent(event)
            when (event.eventType) {
                UsageEvents.Event.SCREEN_INTERACTIVE -> {
                    if (pendingStart == null) {
                        pendingStart = event.timeStamp
                    }
                }
                UsageEvents.Event.SCREEN_NON_INTERACTIVE -> {
                    val s = pendingStart
                    if (s != null) {
                        intervals.add(s to event.timeStamp)
                        pendingStart = null
                    }
                }
            }
        }
        // 問い合わせ時点でまだ画面ONのままの場合は now で一旦区切る。
        pendingStart?.let { intervals.add(it to to) }
        return intervals
    }

    private fun post(baseUrl: String, secret: String, tag: String, tsMs: Long): Boolean {
        return try {
            val url = URL("${baseUrl.trimEnd('/')}?secret=$secret&tag=$tag&ts=$tsMs")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                doOutput = true
                setFixedLengthStreamingMode(0)
                connectTimeout = 15_000
                readTimeout = 15_000
            }
            conn.connect()
            val code = conn.responseCode
            conn.disconnect()
            code in 200..299
        } catch (e: Exception) {
            Log.e(TAG, "[app] UsageReporter: POST $tag ERROR ${e.message}")
            false
        }
    }

    // Rust側のdata_dir()（= Context.filesDir）と同じ場所を見に行く。
    private fun readConfig(context: Context): org.json.JSONObject? {
        return try {
            val filesPath = java.io.File(context.filesDir, "config.json")
            if (!filesPath.exists()) return null
            org.json.JSONObject(filesPath.readText())
        } catch (e: Exception) {
            Log.e(TAG, "[app] UsageReporter: readConfig ERROR ${e.message}")
            null
        }
    }
}
