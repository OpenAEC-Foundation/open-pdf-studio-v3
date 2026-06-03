fn main() {
    slint_build::compile("ui/app.slint").expect("Slint UI compilation failed");

    // Embed the app icon into the Windows .exe as a PE resource at compile time
    // so the installed binary shows the icon in Explorer/taskbar (cargo-packager
    // only sets the installer/shortcut icon). Done at build time so it lands
    // before code-signing and the signature covers the icon-bearing exe.
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/icon.ico");
        // Friendly name shown by Windows (e.g. "Open with → Open PDF Studio").
        res.set("ProductName", "Open PDF Studio");
        res.set("FileDescription", "Open PDF Studio");
        res.compile().expect("failed to embed Windows icon resource");
    }
}
