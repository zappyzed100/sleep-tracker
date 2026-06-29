package com.sleeptracker.app

import android.content.Context
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
        try {
            val config = readConfig() ?: return@withContext Result.success()
            val baseUrl = config.optString("mobile_url").trim()
            val secret  = config.optString("mobile_secret").trim()
            if (baseUrl.isEmpty() || secret.isEmpty()) return@withContext Result.success()

            val ts   = System.currentTimeMillis()
            val url  = URL("${baseUrl.trimEnd('/')}?secret=$secret")
            val conn = (url.openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                setRequestProperty("Content-Type", "text/plain")
                doOutput      = true
                connectTimeout = 15_000
                readTimeout    = 15_000
            }
            conn.outputStream.use { it.write("SCREEN_ON,$ts".toByteArray()) }
            conn.responseCode   // trigger request
            conn.disconnect()
            Result.success()
        } catch (_: Exception) {
            Result.success()    // best-effort; never retry
        }
    }

    private fun readConfig(): JSONObject? = try {
        val f = java.io.File(applicationContext.filesDir, "config.json")
        if (f.exists()) JSONObject(f.readText()) else null
    } catch (_: Exception) { null }
}
