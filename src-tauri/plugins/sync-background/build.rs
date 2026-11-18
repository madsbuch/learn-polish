// Commands are implemented in Kotlin (android/) and called from the app's
// Rust code only, never from the web view.
const COMMANDS: &[&str] = &["schedule", "cancel"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
