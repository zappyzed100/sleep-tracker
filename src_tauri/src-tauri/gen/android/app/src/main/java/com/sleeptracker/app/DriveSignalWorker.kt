package com.sleeptracker.app

import android.content.Context
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

// Sends a SCREEN_ON heartbeat to Google Drive every 15 minutes while the device is active.
// Runs via WorkManager so it works even when the app is in the background.
class DriveSignalWorker(ctx: Context, params: WorkerParameters) : CoroutineWorker(ctx, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        Log.i("SleepTracker", "[worker] DriveSignalWorker: started")
        try {
            val config = readConfig()
            if (config == null) {
                Log.w("SleepTracker", "[worker] DriveSignalWorker: config not found — skip")
                return@withContext Result.success()
            }
            val baseUrl = config.optString("mobile_url").trim()
            val secret  = config.optString("mobile_secret").trim()
            if (baseUrl.isEmpty() || secret.isEmpty()) {
                Log.w("SleepTracker", "[worker] DriveSignalWorker: mobile_url or secret empty — skip")
                return@withContext Result.success()
            }

            val ts = System.currentTimeMillis()
            // Distinguish startup run (WorkManager fires immediately after first schedule)
            // from genuine background heartbeats. Within 5 min of app open → APP_OPEN.
            val openTimeMs = applicationContext
                .getSharedPreferences("sleep_tracker", android.content.Context.MODE_PRIVATE)
                .getLong("app_open_time_ms", 0L)
            val elapsedMs = if (openTimeMs > 0L) ts - openTimeMs else Long.MAX_VALUE
            val tag = if (elapsedMs < 5 * 60 * 1000L) "APP_OPEN" else "SCREEN_ON"
            Log.i("SleepTracker", "[worker] DriveSignalWorker: tag=$tag elapsed=${elapsedMs}ms")

            val url  = URL("${baseUrl.trimEnd('/')}?secret=$secret&tag=$tag&ts=$ts")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod  = "POST"
                doOutput       = true
                setFixedLengthStreamingMode(0)  // Content-Length: 0 (required by GAS)
                connectTimeout = 15_000
                readTimeout    = 15_000
            }
            conn.connect()
            val code = conn.responseCode
            conn.disconnect()
            Log.i("SleepTracker", "[worker] DriveSignalWorker: done (HTTP $code tag=$tag)")
            Result.success()
        } catch (e: Exception) {
            Log.e("SleepTracker", "[worker] DriveSignalWorker: ERROR ${e.message}")
            Result.success()    // best-effort; never retry
        }
    }

    private fun readConfig(): JSONObject? = try {
        // Tauri's app_data_dir() = filesDir (not dataDir)
        // Try filesDir first, then fall back to dataDir for older builds.
        val filesPath = java.io.File(applicationContext.filesDir, "config.json")
        val dataPath  = java.io.File(applicationContext.dataDir, "config.json")
        val f = when {
            filesPath.exists() -> { Log.i("SleepTracker", "[worker] config found: filesDir/${filesPath.name}"); filesPath }
            dataPath.exists()  -> { Log.i("SleepTracker", "[worker] config found: dataDir/${dataPath.name}"); dataPath }
            else -> {
                Log.w("SleepTracker", "[worker] config not found: filesDir=${filesPath.absolutePath} dataDir=${dataPath.absolutePath}")
                return null
            }
        }
        JSONObject(f.readText())
    } catch (e: Exception) {
        Log.e("SleepTracker", "[worker] readConfig ERROR: ${e.message}")
        null
    }
}
