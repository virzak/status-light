fn main() {
    // esp-hal's linker script. Must be referenced for a bootable image, and must
    // come last so it does not clash with other scripts.
    println!("cargo:rustc-link-arg=-Tlinkall.x");
    // Code and data share RAM on the ESP32-S3, so the RAM segment is RWX. That is
    // expected here; silence the linker's warning about it.
    println!("cargo:rustc-link-arg=-Wl,--no-warn-rwx-segments");
}
