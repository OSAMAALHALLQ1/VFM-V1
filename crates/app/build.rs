fn main() {
    slint_build::compile("ui/root.slint").expect("compile Slint UI root");

    #[cfg(windows)]
    {
        // Embed the original app artwork into every produced .exe.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rc_dir = root.join("third_party/windows-sdk-headers/c/bin/10.0.26100.0/x64");
        let mut resource = winres::WindowsResource::new();
        resource.set_toolkit_path(rc_dir.to_str().expect("UTF-8 rc path"));
        resource.set_icon("../../assets/app-icon.ico");
        resource.compile().expect("embed application icon");
    }
}
