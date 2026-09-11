fn main() {
    tauri_build::build();
    
    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    {
        // Set the subsystem to windows to avoid console window (release builds only)
        println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS,5.01");
    }
}
