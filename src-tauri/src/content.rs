use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ClipboardImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl ClipboardImage {
    pub fn from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Option<Self> {
        let pixel_count = (width as usize).checked_mul(height as usize)?;
        let byte_count = pixel_count.checked_mul(4)?;
        if width == 0 || height == 0 || rgba.len() != byte_count {
            return None;
        }

        Some(Self {
            rgba,
            width,
            height,
        })
    }
}

#[derive(Debug, Default)]
pub struct ClipboardSnapshot {
    pub image: Option<ClipboardImage>,
    pub text: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize)]
pub struct ClipboardItem {
    pub text: String,
    pub hash: String,
    #[serde(skip)]
    pub image: Option<ClipboardImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone)]
pub enum PasteContent {
    Text(String),
    Image {
        image: ClipboardImage,
        fallback_text: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct HistorySummary {
    pub text: String,
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

const THUMBNAIL_MAX_SIZE: u32 = 20;

pub fn generate_preview(image: &ClipboardImage) -> Option<String> {
    let (new_w, new_h) = if image.width > THUMBNAIL_MAX_SIZE || image.height > THUMBNAIL_MAX_SIZE {
        if image.width > image.height {
            let h = (image.height * THUMBNAIL_MAX_SIZE).max(1) / image.width;
            (THUMBNAIL_MAX_SIZE, h.max(1))
        } else if image.height > image.width {
            let w = (image.width * THUMBNAIL_MAX_SIZE).max(1) / image.height;
            (w.max(1), THUMBNAIL_MAX_SIZE)
        } else {
            (THUMBNAIL_MAX_SIZE, THUMBNAIL_MAX_SIZE)
        }
    } else {
        (image.width, image.height)
    };

    let mut png_bytes = Vec::new();
    {
        use image::imageops::FilterType;
        use image::ImageFormat;
        use image::RgbaImage;
        use std::io::Cursor;

        let img = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
        let thumb = image::imageops::resize(&img, new_w, new_h, FilterType::Nearest);
        thumb
            .write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png)
            .ok()?;
    }

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);

    Some(format!("data:image/png;base64,{b64}"))
}
