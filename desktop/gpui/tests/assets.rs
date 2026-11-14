use gpui_kit::{assets::AllAssets, AssetSource};
use std::borrow::Cow;

#[test]
fn icons_are_embedded_even_in_debug_builds() {
    let assets = AllAssets;
    let names = assets.list("icons/").unwrap();
    // Check a known icon too: an empty catalog mustn't make the loop pass.
    assert!(names.iter().any(|name| name.as_ref() == "icons/plus.svg"));
    for name in names {
        let bytes = assets.load(name.as_ref()).unwrap().unwrap();
        // RustEmbed reads an owned buffer from disk in its default debug mode.
        // Borrowed bytes prove these survive copying the binary off its host.
        assert!(matches!(bytes, Cow::Borrowed(_)), "{name} came from disk");
        assert!(std::str::from_utf8(&bytes).unwrap().contains("<svg"), "{name}");
    }
}
