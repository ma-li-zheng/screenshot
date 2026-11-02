use std::borrow::Cow;
use std::io::Cursor;

use image::{ImageFormat, RgbaImage};

pub fn decode_png(png: &[u8]) -> anyhow::Result<RgbaImage> {
    Ok(image::load_from_memory_with_format(png, ImageFormat::Png)?.to_rgba8())
}

pub fn encode_png(img: &RgbaImage) -> anyhow::Result<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

pub fn png_size(png: &[u8]) -> anyhow::Result<(u32, u32)> {
    Ok(image::ImageReader::with_format(Cursor::new(png), ImageFormat::Png).into_dimensions()?)
}

pub fn set_image(img: &RgbaImage) -> anyhow::Result<()> {
    let mut cb = arboard::Clipboard::new()?;
    cb.set_image(arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: Cow::Borrowed(img.as_raw()),
    })?;
    Ok(())
}

pub fn set_png(png: &[u8]) -> anyhow::Result<()> {
    set_image(&decode_png(png)?)
}

pub fn set_text(text: &str) -> anyhow::Result<()> {
    arboard::Clipboard::new()?.set_text(text.to_string())?;
    Ok(())
}

pub fn get_image() -> Option<RgbaImage> {
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
}
