// Encoding the scripted window's pixels, away from the app's thread.
use image::{codecs::png::{CompressionType, FilterType, PngEncoder}, ImageEncoder, ImageFormat, RgbaImage};
use std::io::Cursor;
use std::path::Path;

pub(crate) fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        let format = ImageFormat::from_path(path)?;
        let mut encoded = Cursor::new(Vec::new());
        if format == ImageFormat::Png {
            // Adaptive filtering tries every filter for every row. In a
            // debug app at background priority this took 30 seconds for
            // one window (14 Nov), longer than the caller's timeout.
            // Sub keeps all pixels and compresses UI's flat areas well.
            PngEncoder::new_with_quality(&mut encoded, CompressionType::Fast, FilterType::Sub)
                .write_image(image.as_raw(), image.width(), image.height(), image::ExtendedColorType::Rgba8)?;
        } else {
            image.write_to(&mut encoded, format)?;
        }
        // Finish encoding before opening the destination. Normal writes
        // keep its permissions and write through symlinks and hard links;
        // renaming a tempfile would replace links and reset permissions.
        std::fs::write(path, encoded.into_inner())?;
        Ok(())
    };
    save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels() -> RgbaImage {
        RgbaImage::from_fn(127, 83, |x, y| image::Rgba([x as u8, y as u8, (x * y) as u8, (x + y) as u8]))
    }

    #[test]
    fn a_completed_png_has_every_pixel_including_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.png");
        let image = pixels();
        save(&image, &path).unwrap();
        assert_eq!(image::open(path).unwrap().to_rgba8(), image);
    }

    #[test]
    fn a_codec_error_leaves_an_old_screenshot_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.png");
        std::fs::write(&path, b"old image").unwrap();
        save(&pixels(), &path).unwrap();
        assert_eq!(image::open(path).unwrap().to_rgba8(), pixels());
        // JPEG cannot encode RGBA. A codec error must return an
        // error and leave the old file alone, never make it look like a shot.
        let path = dir.path().join("window.jpg");
        std::fs::write(&path, b"old image").unwrap();
        assert!(save(&pixels(), &path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"old image");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_missing_parent_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save(&pixels(), &dir.path().join("missing/window.png")).is_err());
    }

    #[test]
    fn a_write_error_does_not_leave_extra_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.png");
        std::fs::create_dir(&path).unwrap();
        assert!(save(&pixels(), &path).is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn new_shots_follow_the_umask_and_existing_shots_keep_their_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let control = dir.path().join("control");
        std::fs::write(&control, b"normal file").unwrap();
        let path = dir.path().join("window.png");
        save(&pixels(), &path).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), mode(&control));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        save(&pixels(), &path).unwrap();
        assert_eq!(mode(&path), 0o640);
    }

    #[cfg(unix)]
    #[test]
    fn a_shot_writes_through_a_symlink_even_when_its_target_is_new() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("served.png");
        let link = dir.path().join("window.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        for _ in 0..2 {
            save(&pixels(), &link).unwrap();
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(image::open(&target).unwrap().to_rgba8(), pixels());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_shot_keeps_hard_links_pointing_at_the_updated_image() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("served.png");
        let link = dir.path().join("window.png");
        std::fs::write(&target, b"old image").unwrap();
        std::fs::hard_link(&target, &link).unwrap();
        save(&pixels(), &link).unwrap();
        assert_eq!(std::fs::metadata(&target).unwrap().ino(), std::fs::metadata(&link).unwrap().ino());
        assert_eq!(image::open(&target).unwrap().to_rgba8(), pixels());
    }
}
