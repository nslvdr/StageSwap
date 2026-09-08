use image::imageops::FilterType;

const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/app-icon.png");

pub struct AppIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn load(size: Option<u32>) -> Result<AppIcon, String> {
    let image = image::load_from_memory(APP_ICON_PNG)
        .map_err(|error| format!("could not decode embedded app icon: {error}"))?;
    let image = match size {
        Some(size) => image.resize_exact(size, size, FilterType::Lanczos3),
        None => image,
    };
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(AppIcon {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_canonical_icon_is_square_rgba_with_transparent_corners() {
        let icon = load(None).expect("embedded icon should decode");
        assert_eq!((icon.width, icon.height), (512, 512));
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        for (x, y) in [(0, 0), (511, 0), (0, 511), (511, 511)] {
            let alpha = icon.rgba[((y * icon.width + x) * 4 + 3) as usize];
            assert_eq!(alpha, 0, "corner ({x}, {y}) should be transparent");
        }
        let opaque_pixels = icon
            .rgba
            .chunks_exact(4)
            .filter(|pixel| pixel[3] == 255)
            .count();
        assert!(opaque_pixels > 512 * 512 / 2);
        let icon = load(Some(32)).expect("embedded tray icon should decode");
        assert_eq!((icon.width, icon.height), (32, 32));
        assert_eq!(icon.rgba.len(), 32 * 32 * 4);
    }
}
