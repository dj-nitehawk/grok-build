use super::placeholder_images::{PlaceholderLoadError, load_placeholder_image};
use image::{DynamicImage, ImageFormat};

#[test]
fn placeholder_loader_keeps_supported_codecs_without_tools() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = [dunce::canonicalize(dir.path()).unwrap()];
    let base = [
        (ImageFormat::Png, "png", "image/png"),
        (ImageFormat::Jpeg, "jpg", "image/jpeg"),
    ];
    #[cfg(feature = "image-extra")]
    let extra = [
        (ImageFormat::Gif, "gif", "image/gif"),
        (ImageFormat::WebP, "webp", "image/webp"),
        (ImageFormat::Bmp, "bmp", "image/bmp"),
        (ImageFormat::Tiff, "tiff", "image/tiff"),
    ];
    #[cfg(not(feature = "image-extra"))]
    let extra: [(ImageFormat, &str, &str); 0] = [];
    for (format, extension, mime) in base.into_iter().chain(extra) {
        let path = dir.path().join(format!("image.{extension}"));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::new_rgb8(8, 4)
            .write_to(&mut bytes, format)
            .unwrap();
        let bytes = bytes.into_inner();
        std::fs::write(&path, &bytes).unwrap();

        let loaded = load_placeholder_image(path.to_str().unwrap(), &allowed).unwrap();
        assert_eq!((loaded.mime_type.as_str(), loaded.data), (mime, bytes));
    }
}

#[cfg(not(feature = "image-extra"))]
#[test]
fn placeholder_loader_rejects_gif_extension_when_codec_off() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = [dunce::canonicalize(dir.path()).unwrap()];
    let path = dir.path().join("anim.gif");
    std::fs::write(&path, b"GIF89a").unwrap();
    assert!(matches!(
        load_placeholder_image(path.to_str().unwrap(), &allowed),
        Err(PlaceholderLoadError::UnsupportedExtension)
    ));
}

#[cfg(feature = "image-extra")]
#[test]
fn placeholder_loader_rejects_disallowed_codec_behind_allowed_extension() {
    let dir = tempfile::tempdir().unwrap();
    let allowed = [dunce::canonicalize(dir.path()).unwrap()];
    let path = dir.path().join("disguised.png");
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::new_rgba8(8, 4)
        .write_to(&mut bytes, ImageFormat::Ico)
        .unwrap();
    std::fs::write(&path, bytes.into_inner()).unwrap();

    assert!(matches!(
        load_placeholder_image(path.to_str().unwrap(), &allowed),
        Err(PlaceholderLoadError::NotAnImage)
    ));
}
