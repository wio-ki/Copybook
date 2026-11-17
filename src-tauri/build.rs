fn main() {
    tauri_build::build();
    // Integration tests that use Tauri's mock runtime still import TaskDialogIndirect.
    // Give those test executables the same common-controls v6 activation as the app.
    println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
}
