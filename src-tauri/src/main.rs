// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--guard") {
        let plan = args.get(i + 1).cloned().unwrap_or_default();
        std::process::exit(haul_lib::guard_main(&plan));
    }
    haul_lib::run()
}
