package com.madsbuch.learnpolish.syncbg

import android.app.Activity
import androidx.work.Constraints
import androidx.work.BackoffPolicy
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.util.concurrent.TimeUnit

@InvokeArg
class ScheduleArgs {
    var dbPath: String = ""
}

/**
 * Schedules the periodic background sync. WorkManager keeps the schedule
 * across reboots and app updates, runs it only on unmetered networks (home
 * Wi-Fi, where the computer is), and spaces it out under Doze as it sees fit.
 */
@TauriPlugin
class SyncBackgroundPlugin(private val activity: Activity) : Plugin(activity) {

    @Command
    fun schedule(invoke: Invoke) {
        val args = invoke.parseArgs(ScheduleArgs::class.java)
        if (args.dbPath.isBlank()) {
            invoke.reject("dbPath is required")
            return
        }
        val request = PeriodicWorkRequestBuilder<SyncWorker>(30, TimeUnit.MINUTES)
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.UNMETERED).build(),
            )
            .setInputData(workDataOf(SyncWorker.KEY_DB_PATH to args.dbPath))
            .build()
        // UPDATE keeps the schedule's timing when the app starts again, and
        // takes a new database path if there is one.
        WorkManager.getInstance(activity.applicationContext).enqueueUniquePeriodicWork(
            SyncWorker.NAME,
            ExistingPeriodicWorkPolicy.UPDATE,
            request,
        )
        invoke.resolve()
    }

    /**
     * The job that finishes downloading the courses kept offline if the app
     * is stopped first ([OfflineWorker]). On any network: the learner asked
     * for the course, and the app downloads on any network too. Expedited,
     * so it starts soon after the app is gone; while the app is still
     * downloading it asks to be run again, a little later each time. One at
     * a time: asked again while one waits or runs, that one stays.
     */
    @Command
    fun scheduleOffline(invoke: Invoke) {
        val args = invoke.parseArgs(ScheduleArgs::class.java)
        if (args.dbPath.isBlank()) {
            invoke.reject("dbPath is required")
            return
        }
        val request = OneTimeWorkRequestBuilder<OfflineWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setBackoffCriteria(BackoffPolicy.LINEAR, 30, TimeUnit.SECONDS)
            .setInputData(workDataOf(OfflineWorker.KEY_DB_PATH to args.dbPath))
            .build()
        WorkManager.getInstance(activity.applicationContext).enqueueUniqueWork(
            OfflineWorker.NAME,
            ExistingWorkPolicy.KEEP,
            request,
        )
        invoke.resolve()
    }

    @Command
    fun cancel(invoke: Invoke) {
        WorkManager.getInstance(activity.applicationContext).cancelUniqueWork(SyncWorker.NAME)
        invoke.resolve()
    }
}
