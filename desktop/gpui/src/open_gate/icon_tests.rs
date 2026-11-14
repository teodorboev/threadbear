use super::{icon_png, read_icon, Origin};
use std::path::Path;

fn decoded(name: &str) -> image::RgbaImage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons").join(name);
    let checked = read_icon(&path, Origin::ThisMac).unwrap();
    let png = icon_png(checked).unwrap();
    image::load_from_memory(&png).unwrap().to_rgba8()
}

#[test]
fn a_small_pdf_icon_fills_the_rail_image_and_uses_the_crop_box() {
    let image = decoded("Small.pdf");
    assert_eq!(image.dimensions(), (128, 128));
    let filled = image.pixels().filter(|p| p[3] > 240 && p[0] > 200).count();
    assert!(filled > 16_000, "The vector icon filled only {filled} pixels");
}

#[test]
fn a_rotated_pdf_icon_keeps_its_shape_and_clockwise_rotation() {
    let image = decoded("Rotated.pdf");
    assert!(image.get_pixel(64, 8)[3] < 10, "The rotated rectangle should have padding above it");
    let left = image.get_pixel(8, 64);
    let right = image.get_pixel(120, 64);
    assert!(left[2] > 200 && left[0] < 80, "The original bottom half should rotate left: {left:?}");
    assert!(right[0] > 200 && right[2] < 80, "The original top half should rotate right: {right:?}");
}

#[test]
fn an_icns_uses_the_large_blue_representation_instead_of_the_small_red_one() {
    let image = decoded("Multi.icns");
    let center = image.get_pixel(64, 64);
    assert!(center[2] > 220 && center[0] < 80, "The large representation is blue: {center:?}");
}

#[test]
fn an_icon_from_another_mac_never_reads_the_same_local_path() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons/Small.pdf");
    assert!(read_icon(&path, Origin::OtherMac).is_err());
}

#[test]
fn an_icns_cannot_hide_an_enormous_png_behind_its_representation_type() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons/Huge.icns");
    let checked = read_icon(&path, Origin::ThisMac).unwrap();
    assert!(icon_png(checked).is_err());
}

/// A made-up application bundle under a temp folder of its own: an
/// Info.plist naming `icon`, claiming Chrome's id as Daybreak 112's did, and
/// Multi.icns in its Resources unless `with_icon` is false.
struct Bundle(std::path::PathBuf);
impl Drop for Bundle {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(self.0.parent().unwrap()); }
}
fn bundle(tag: &str, icon: &str, with_icon: bool) -> Bundle {
    let folder = std::env::temp_dir().join(format!("tb-app-icon-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let app = folder.join("Impostor.app");
    std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    threadbear_core::private_file::write(&app.join("Contents/Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.google.Chrome</string><key>CFBundleExecutable</key><string>run</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleIconFile</key><string>{icon}</string></dict></plist>"#).as_bytes()).unwrap();
    threadbear_core::private_file::write(&app.join("Contents/MacOS/run"), b"#!/bin/sh\nexit 0\n").unwrap();
    if with_icon {
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons/Multi.icns"), app.join("Contents/Resources/Multi.icns")).unwrap();
    }
    Bundle(app)
}

/// An application's icon is its icns file, read from inside its bundle and
/// drawn on a worker, never through NSWorkspace (Daybreak 112).
#[test]
fn an_application_icon_is_its_own_icns_read_off_the_main_thread() {
    let app = bundle("plain", "Multi", true);
    let png = std::thread::spawn(move || {
        let checked = read_icon(&app.0, Origin::ThisMac).unwrap();
        icon_png(checked).unwrap()
    }).join().unwrap();
    let image = image::load_from_memory(&png).unwrap().to_rgba8();
    let center = image.get_pixel(64, 64);
    assert!(center[2] > 220 && center[0] < 80, "The large representation is blue: {center:?}");
    let named = bundle("named", "Multi.icns", true);
    assert!(read_icon(&named.0, Origin::ThisMac).is_ok(), "a name with its extension");
}

/// An icon file that climbs out, is a link out, or isn't there, is no icon:
/// the patch shows its symbol.
#[test]
fn an_application_icon_never_leaves_its_bundle() {
    let climbs = bundle("climbs", "../../../../Multi", true);
    assert!(read_icon(&climbs.0, Origin::ThisMac).is_err());
    let missing = bundle("missing", "Multi", false);
    assert!(read_icon(&missing.0, Origin::ThisMac).is_err());
    let linked = bundle("linked", "Multi", false);
    std::os::unix::fs::symlink(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons/Multi.icns"), linked.0.join("Contents/Resources/Multi.icns")).unwrap();
    assert!(read_icon(&linked.0, Origin::ThisMac).is_err(), "a link to an icns outside the bundle");
    let plist = bundle("plist", "Multi", true);
    std::fs::remove_file(plist.0.join("Contents/Info.plist")).unwrap();
    std::os::unix::fs::symlink(Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/icons/Small.pdf"), plist.0.join("Contents/Info.plist")).unwrap();
    assert!(read_icon(&plist.0, Origin::ThisMac).is_err(), "an Info.plist linked from outside");
}

/// Reading an impostor's icon leaves Launch Services as it was: Chrome's id
/// still finds what it found before, never the bundle just drawn.
#[test]
fn reading_an_impostors_icon_does_not_register_it() {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;
    let app = bundle("register", "Multi", true);
    let real = std::fs::canonicalize(&app.0).unwrap();
    let _ = icon_png(read_icon(&app.0, Origin::ThisMac).unwrap()).unwrap();
    let id = NSString::from_str(threadbear_core::reply_links::CHROME_BUNDLE_ID);
    let urls = NSWorkspace::sharedWorkspace().URLsForApplicationsWithBundleIdentifier(&id);
    for i in 0..urls.count() {
        let path = urls.objectAtIndex(i).path().map(|p| p.to_string()).unwrap_or_default();
        assert!(!path.starts_with(&*real.to_string_lossy()), "the impostor was registered: {path}");
    }
}

/// The Chrome launcher's check: an unsigned bundle with Chrome's id fails
/// it, and the installed Chrome (where there is one) passes.
#[test]
fn only_googles_chrome_passes_the_chrome_check() {
    let app = bundle("chrome", "Multi", true);
    assert!(super::signed_as(&app.0.to_string_lossy(), super::GOOGLE_CHROME).is_err());
    assert!(super::start_chrome_profile(&app.0.to_string_lossy(), "https://example.com", "Default").is_err());
    // Apple's own app, where apps are installed, passes; a link to it from
    // a temp folder is followed to it. Anywhere else isn't started however
    // it's signed: a copy in a folder something else writes could be
    // swapped between the check and `open`.
    let calculator = Path::new("/System/Applications/Calculator.app");
    let requirement = "anchor apple and identifier \"com.apple.calculator\"";
    assert!(super::signed_as(&calculator.to_string_lossy(), requirement).is_ok());
    let link = app.0.parent().unwrap().join("Calculator.app");
    std::os::unix::fs::symlink(calculator, &link).unwrap();
    assert!(super::signed_as(&link.to_string_lossy(), requirement).is_ok(), "a link is followed to the real app");
    for elsewhere in ["/private/var/folders/x/Calculator.app", "/Users/someone/dev/repo/Calculator.app", "/Users/someone/Applications/Google Chrome.app", "/Applicationsx/Chrome.app"] {
        assert!(!super::installed_where_apps_are(Path::new(elsewhere)), "{elsewhere}");
    }
    for installed in ["/Applications/Google Chrome.app", "/Applications/Xcode.app/Contents/Developer/Applications/Simulator.app", "/System/Applications/Calculator.app"] {
        assert!(super::installed_where_apps_are(Path::new(installed)), "{installed}");
    }
    let installed = Path::new("/Applications/Google Chrome.app");
    if installed.exists() {
        assert!(super::signed_as(&installed.to_string_lossy(), super::GOOGLE_CHROME).is_ok(), "Google's own Chrome was refused");
    }
}
