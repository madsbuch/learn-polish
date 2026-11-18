# The plugin's commands are found by reflection, WorkManager makes the worker
# by its class name, and SyncNative's native method is bound by its name.
-keep class com.madsbuch.learnpolish.syncbg.** { *; }
