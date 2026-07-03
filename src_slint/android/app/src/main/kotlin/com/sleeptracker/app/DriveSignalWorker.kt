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

// Sends a DEVICE_ON heartbeat to Google Drive every 15 minutes while the device is active.
// Runs via WorkManager so it works even when the app is in the background.
// Tauri版 DriveSignalWorker.kt からほぼそのまま移植（Rust側を呼ばずKotlin単体で完結する設計）。
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

            val ts  = System.currentTimeMillis()
            val tag = "DEVICE_ON"

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

    // Rust側のdata_dir()（= AndroidApp::internal_data_path() = Context.filesDir）と
    // 同じ場所を見に行く。
    private fun readConfig(): JSONObject? {
        return try {
            val filesPath = java.io.File(applicationContext.filesDir, "config.json")
            if (!filesPath.exists()) {
                Log.w("SleepTracker", "[worker] config not found: ${filesPath.absolutePath}")
                return null
            }
            JSONObject(filesPath.readText())
        } catch (e: Exception) {
            Log.e("SleepTracker", "[worker] readConfig ERROR: ${e.message}")
            null
        }
    }
}
