// The Mac's own video and PDF views (native/media_views.m): AVKit and PDFKit
// are Objective-C, so their small shim is compiled here rather than written
// as objc2 calls, as the web runtime's AppKit glue is (../web/build.rs).
fn main() {
    println!("cargo:rerun-if-changed=native/media_views.m");
    println!("cargo:rerun-if-changed=native/menu_icons.m");
    println!("cargo:rerun-if-changed=native/file_icons.m");
    println!("cargo:rerun-if-changed=native/composer_media.m");
    println!("cargo:rerun-if-changed=native/translation.swift");
    println!("cargo:rerun-if-changed=native/text_selection.h");
    println!("cargo:rerun-if-changed=native/text_selection.m");
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
        cc::Build::new().file("native/menu_icons.m").flag("-fobjc-arc").compile("threadbear_menu_icons");
        cc::Build::new().file("native/file_icons.m").flag("-fobjc-arc").compile("threadbear_file_icons");
        let mut text = cc::Build::new(); text.file("native/text_selection.m").flag("-fobjc-arc").flag("-fobjc-arc-exceptions").flag("-Wno-deprecated-declarations"); if std::env::var_os("CARGO_FEATURE_AUTOMATION").is_some() { text.define("TB_TEXT_TEST", None); } text.compile("threadbear_text_selection");
        // SwiftUI exposes the system Translate popover for custom content.
        // This small anchor has no editor; all text/replacement stays behind
        // the same frozen selection checks as Services. Link its static object
        // against the OS Swift runtime, never a copied runtime or dylib.
        use std::{path::PathBuf, process::Command};
        let sdk = Command::new("xcrun").args(["--sdk", "macosx", "--show-sdk-path"]).output().expect("find macOS SDK");
        assert!(sdk.status.success(), "find macOS SDK");
        let sdk = String::from_utf8(sdk.stdout).expect("SDK path");
        let sdk = sdk.trim();
        let compiler = Command::new("xcrun").args(["--find", "swiftc"]).output().expect("find swiftc");
        assert!(compiler.status.success(), "find swiftc");
        let compiler = String::from_utf8(compiler.stdout).expect("swiftc path");
        let compiler = PathBuf::from(compiler.trim());
        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        let object = out.join("translation.o");
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() { Ok("aarch64") => "arm64", Ok("x86_64") => "x86_64", other => panic!("unsupported Mac architecture: {other:?}") };
        let status = Command::new(&compiler).args(["-parse-as-library", "-emit-object", "-O", "-import-objc-header", "native/text_selection.h", "-module-name", "ThreadbearTranslation", "-target", &format!("{arch}-apple-macosx13.0"), "-sdk", sdk, "-module-cache-path"]).arg(out.join("swift-modules"))
            .arg("native/translation.swift").arg("-o").arg(&object).status().expect("compile Translate anchor");
        assert!(status.success(), "compile Translate anchor");
        cc::Build::new().object(&object).compile("threadbear_translation");
        println!("cargo:rustc-link-search=native={}", compiler.parent().unwrap().parent().unwrap().join("lib/swift/macosx").display());
        println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
        let mut media = cc::Build::new();
        media.file("native/composer_media.m").flag("-fobjc-arc").flag("-Wno-deprecated-declarations");
        if std::env::var_os("CARGO_FEATURE_AUTOMATION").is_some() { media.define("TB_COMPOSER_AUTOMATION", None); }
        media.compile("threadbear_composer_media");
        for framework in ["AppKit", "Foundation", "AVFoundation", "CoreMedia", "QuickLookThumbnailing", "AVKit", "PDFKit", "ImageIO", "CoreGraphics", "QuickLookThumbnailing", "UniformTypeIdentifiers"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
