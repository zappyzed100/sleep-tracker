package com.sleeptracker.app

import android.app.AppOpsManager
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.Intent
import android.os.Process
import android.provider.Settings
import android.util.Log
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

// タブレットで実際にアプリを使っていた区間（読書・動画視聴等）をUsageStatsManagerから
// 検出し、APP_USAGE_START/APP_USAGE_ENDとしてDriveへ送信する。
//
// 背景：15分ごとのバックグラウンド送信（旧DriveSignalWorkerの定期実行）は
// 「タブレットの電源が入っているか」しか分からず、「実際に使っていたか」の
// 証拠にならなかった（詳細はscratchpad/sync_design_testでの検証を参照）。
// UsageStatsManagerはOS側が常時記録している履歴なので、こちらのアプリが
// バックグラウンドで動いている必要がない。起動・再開時に遡って回収するだけでよい。
//
// フィルタ・統合（短すぎる利用の除外、細切れの統合）はRust側(events.rs)の
// coalesce_and_filter_app_usageが一括で担うため、ここでは検出した区間を
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

    // 前回確認時刻〜現在までのフォアグラウンドアプリ利用区間を集めてDriveへ送信する。
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

        val intervals = queryForegroundIntervals(context, from, now)
        if (intervals.isEmpty()) {
            prefs.edit().putLong(KEY_LAST_QUERY_MS, now).apply()
            Log.i(TAG, "[app] UsageReporter: no usage intervals found")
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
            val okStart = post(baseUrl, secret, "APP_USAGE_START", s)
            val okEnd   = post(baseUrl, secret, "APP_USAGE_END", e)
            if (!okStart || !okEnd) allOk = false
        }
        Log.i(TAG, "[app] UsageReporter: sent ${intervals.size} interval(s), allOk=$allOk")
        prefs.edit().putLong(KEY_LAST_QUERY_MS, now).apply()
    }

    // UsageEvents から「何らかのアプリがフォアグラウンドにあった区間」を再構成する。
    // どのアプリかは問わない（読書アプリでも動画アプリでも同じ扱い）。
    //
    // 数秒単位で不自然に連続するAPP_USAGE区間が実機で観測された（睡眠中にタブレットに
    // 触れていないはずの時間帯に記録される）ため、原因特定用にパッケージ名を全件
    // logcatへ出す。次回同じ現象が起きた際、`adb logcat | grep "\[app\]"` で
    // 何が反応しているか（システムUI・通知・特定アプリ等）を特定できるようにするため
    // （フィルタ・除外ルールはこのログで原因が分かってから追加する）。
    private fun queryForegroundIntervals(context: Context, from: Long, to: Long): List<Pair<Long, Long>> {
        val usm = context.getSystemService(Context.USAGE_STATS_SERVICE) as UsageStatsManager
        val events = usm.queryEvents(from, to)
        val intervals = mutableListOf<Pair<Long, Long>>()
        var pendingStart: Long? = null
        var pendingPkg: String? = null
        val event = UsageEvents.Event()
        while (events.hasNextEvent()) {
            events.getNextEvent(event)
            when (event.eventType) {
                UsageEvents.Event.MOVE_TO_FOREGROUND -> {
                    Log.i(TAG, "[app] queryForegroundIntervals: FOREGROUND pkg=${event.packageName} ts=${event.timeStamp}")
                    if (pendingStart == null) {
                        pendingStart = event.timeStamp
                        pendingPkg = event.packageName
                    }
                }
                UsageEvents.Event.MOVE_TO_BACKGROUND -> {
                    Log.i(TAG, "[app] queryForegroundIntervals: BACKGROUND pkg=${event.packageName} ts=${event.timeStamp}")
                    val s = pendingStart
                    if (s != null) {
                        val durationSec = (event.timeStamp - s) / 1000
                        Log.i(TAG, "[app] queryForegroundIntervals: interval pkg=$pendingPkg ${durationSec}s")
                        intervals.add(s to event.timeStamp)
                        pendingStart = null
                        pendingPkg = null
                    }
                }
            }
        }
        // 問い合わせ時点でまだ何かがフォアグラウンドにある場合は now で一旦区切る。
        pendingStart?.let {
            Log.i(TAG, "[app] queryForegroundIntervals: still-open pkg=$pendingPkg at query time")
            intervals.add(it to to)
        }
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
    private fun readConfig(context: Context): JSONObject? {
        return try {
            val filesPath = java.io.File(context.filesDir, "config.json")
            if (!filesPath.exists()) return null
            JSONObject(filesPath.readText())
        } catch (e: Exception) {
            Log.e(TAG, "[app] UsageReporter: readConfig ERROR ${e.message}")
            null
        }
    }
}
