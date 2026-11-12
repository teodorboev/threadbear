// The AppKit shim CEF needs on a Mac (native/appkit_hooks.m). Objective-C is
// the plain way to give GPUI's NSApplication CefAppProtocol and to place
// native views, so it's compiled here rather than written as objc2 calls.
fn main() {
    println!("cargo:rerun-if-changed=native/appkit_hooks.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("native/appkit_hooks.m").flag("-fobjc-arc").compile("threadbear_web_appkit");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
