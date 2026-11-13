// Commands are implemented in Kotlin (android/); Tauri forwards each call
// there. Listing them here generates the matching permissions.
const COMMANDS: &[&str] = &[
    "set_queue",
    "play",
    "pause",
    "toggle",
    "next",
    "previous",
    "seek_to",
    "skip_to",
    "set_repeat",
    "stop",
    "get_state",
    "remote_start",
    "remote_stop",
    "multicast",
    "register_listener",
    "remove_listener",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
