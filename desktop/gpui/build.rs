// The Mac's own video and PDF views (native/media_views.m): AVKit and PDFKit
// are Objective-C, so their small shim is compiled here rather than written
// as objc2 calls, as the web runtime's AppKit glue is (../web/build.rs).
fn main() {
    println!("cargo:rerun-if-changed=native/media_views.m");
    // The main thread's stack as big as a Mac's (8 MB): Windows gives it 1 MB,
    // and the app overflowed it answering a card with a picture in a
    // Something else box, on GitHub's Windows runner (12 Nov). Zed does the
    // same.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bins=/STACK:8388608");
        } else {
            println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // The older asset-loading calls work on every macOS this runs on;
        // their deprecation warnings aren't worth an availability split.
        cc::Build::new().file("native/media_views.m").flag("-fobjc-arc").flag("-Wno-deprecated-declarations").compile("threadbear_media_views");
        for framework in ["AppKit", "Foundation", "AVFoundation", "AVKit", "PDFKit"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
