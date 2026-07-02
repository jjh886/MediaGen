fn main() {
    println!("cargo:rerun-if-changed=assets/mediagen.ico");
    println!("cargo:rerun-if-changed=assets/mediagen.icns");
    #[cfg(windows)]
    {
        winresource::WindowsResource::new()
            .set_icon("assets/mediagen.ico")
            .compile()
            .expect("failed to embed MediaGen Windows icon");
    }
}
