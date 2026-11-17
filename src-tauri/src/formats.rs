use crate::model::{CmdError, Result};
use image::RgbaImage;

pub fn parse_html(bytes: &[u8]) -> Option<String> {
    let field = |name: &str| -> Option<usize> {
        let header = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
        header
            .lines()
            .find_map(|line| line.strip_prefix(name).and_then(|n| n.trim().parse().ok()))
    };
    if let (Some(start), Some(end)) = (field("StartFragment:"), field("EndFragment:")) {
        if start <= end {
            if let Some(slice) = bytes.get(start..end) {
                if let Ok(s) = std::str::from_utf8(slice) {
                    return Some(s.to_owned());
                }
            }
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let start = text.find("<!--StartFragment-->")? + "<!--StartFragment-->".len();
    let end = text[start..].find("<!--EndFragment-->")? + start;
    Some(text[start..end].to_owned())
}
pub fn build_html(fragment: &str) -> Vec<u8> {
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let header = |a, b, c, d| {
        format!("Version:1.0\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n")
    };
    let start = header(0, 0, 0, 0).len();
    let fs = start + prefix.len();
    let fe = fs + fragment.len();
    let end = fe + suffix.len();
    format!("{}{prefix}{fragment}{suffix}\0", header(start, end, fs, fe)).into_bytes()
}
pub fn html_text(fragment: &str) -> String {
    // HTML-only clipboard fragments commonly contain block and line-break tags.
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in fragment.chars() {
        if c == '<' {
            in_tag = true;
            tag.clear();
        } else if in_tag && c == '>' {
            in_tag = false;
            if ["br", "br/", "/p", "/div", "/li", "/tr"]
                .contains(&tag.trim().to_ascii_lowercase().as_str())
            {
                out.push('\n');
            }
        } else if in_tag {
            tag.push(c);
        } else {
            out.push(c);
        }
    }
    html_escape::decode_html_entities(&out).trim().to_owned()
}
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

pub fn decode_dib(bytes: &[u8]) -> Result<RgbaImage> {
    let invalid = || CmdError::invalid("Unsupported or invalid DIB");
    let header = u32_at(bytes, 0).ok_or_else(invalid)? as usize;
    if header < 40 || header > bytes.len() {
        return Err(invalid());
    }
    let width = u32_at(bytes, 4).ok_or_else(invalid)? as i32;
    let signed_height = u32_at(bytes, 8).ok_or_else(invalid)? as i32;
    let height = signed_height.unsigned_abs();
    let bpp = u16_at(bytes, 14).ok_or_else(invalid)?;
    let compression = u32_at(bytes, 16).ok_or_else(invalid)?;
    if width <= 0
        || height == 0
        || height > 32768
        || width > 32768
        || u16_at(bytes, 12) != Some(1)
        || ![24, 32].contains(&bpp)
        || ![0, 3, 6].contains(&compression)
    {
        return Err(invalid());
    }
    let width = width as u32;
    let stride = (u64::from(width) * u64::from(bpp)).div_ceil(32) * 4;
    let mut offset = header;
    let mut masks = [
        0x00ff0000,
        0x0000ff00,
        0x000000ff,
        if bpp == 32 { 0xff000000 } else { 0 },
    ];
    if compression == 3 || compression == 6 {
        if header == 40 {
            offset += if compression == 6 { 16 } else { 12 };
        }
        for (i, mask) in masks.iter_mut().enumerate() {
            *mask = if i == 3 && header < 56 && compression != 6 {
                0
            } else {
                u32_at(bytes, 40 + i * 4).ok_or_else(invalid)?
            };
        }
        if masks[..3].contains(&0) {
            return Err(invalid());
        }
    }
    let palette = u32_at(bytes, 32).unwrap_or(0) as usize;
    offset = offset
        .checked_add(palette.checked_mul(4).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    let len = stride.checked_mul(u64::from(height)).ok_or_else(invalid)?;
    if len > 800 * 1024 * 1024
        || (offset as u64 + len) > bytes.len() as u64
        || u64::from(width) * u64::from(height) > 100_000_000
    {
        return Err(invalid());
    }
    let mut result = RgbaImage::new(width, height);
    for y in 0..height {
        let row = if signed_height < 0 { y } else { height - 1 - y };
        for x in 0..width {
            let pos = offset + (u64::from(row) * stride) as usize + x as usize * (bpp as usize / 8);
            let pixel = if bpp == 24 {
                u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], 0])
            } else {
                u32_at(bytes, pos).ok_or_else(invalid)?
            };
            let channel = |mask: u32| -> u8 {
                if mask == 0 {
                    return 255;
                }
                let shift = mask.trailing_zeros();
                let max = mask >> shift;
                (((pixel & mask) >> shift) as u64 * 255 / u64::from(max)) as u8
            };
            result.put_pixel(x, y, image::Rgba(masks.map(channel)));
        }
    }
    if bpp == 32 && result.pixels().all(|p| p[3] == 0) {
        for p in result.pixels_mut() {
            p[3] = 255;
        }
    }
    Ok(result)
}
pub fn encode_dib(pixels: &RgbaImage) -> Vec<u8> {
    let mut bytes = vec![0u8; 40];
    bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&pixels.width().to_le_bytes());
    bytes[8..12].copy_from_slice(&(-(pixels.height() as i32)).to_le_bytes());
    bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&32u16.to_le_bytes());
    for p in pixels.pixels() {
        bytes.extend([p[2], p[1], p[0], p[3]]);
    }
    bytes
}

/// BITMAPV5HEADER with explicit RGBA masks, readable by native CLI clipboard libraries.
pub fn encode_dibv5(pixels: &RgbaImage) -> Vec<u8> {
    let dib = encode_dib(pixels);
    let mut bytes = vec![0u8; 124];
    bytes[..40].copy_from_slice(&dib[..40]);
    bytes[..4].copy_from_slice(&124u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    bytes[20..24].copy_from_slice(&(dib.len() as u32 - 40).to_le_bytes());
    for (offset, mask) in [
        (40, 0x00ff0000u32),
        (44, 0x0000ff00),
        (48, 0x000000ff),
        (52, 0xff000000),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&mask.to_le_bytes());
    }
    bytes[56..60].copy_from_slice(&0x73524742u32.to_le_bytes()); // LCS_sRGB
    bytes[108..112].copy_from_slice(&4u32.to_le_bytes()); // LCS_GM_IMAGES
    bytes.extend_from_slice(&dib[40..]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cf_html_utf8_offsets_and_markers() {
        let fragment = "<b>中文 😀</b>";
        assert_eq!(parse_html(&build_html(fragment)).as_deref(), Some(fragment));
        assert_eq!(
            parse_html(b"junk<!--StartFragment-->abc<!--EndFragment-->").as_deref(),
            Some("abc")
        );
        assert_eq!(html_text("<p>A &amp; B</p><br>中文"), "A & B\n\n中文");
    }
    #[test]
    fn dib_rows_formats_alpha_and_bitfields() {
        let image = RgbaImage::from_fn(2, 2, |x, y| {
            image::Rgba([x as u8 * 90, y as u8 * 80, 30, 255])
        });
        let dib = encode_dib(&image);
        assert_eq!(decode_dib(&dib).unwrap(), image);
        for bpp in [24u16, 32] {
            for top_down in [false, true] {
                let stride = (2usize * bpp as usize).div_ceil(32) * 4;
                let mut data = dib[..40].to_vec();
                data[14..16].copy_from_slice(&bpp.to_le_bytes());
                data[8..12].copy_from_slice(&(if top_down { -2i32 } else { 2i32 }).to_le_bytes());
                for row in 0..2 {
                    let y = if top_down { row } else { 1 - row };
                    let start = data.len();
                    for x in 0..2 {
                        let p = image.get_pixel(x, y);
                        data.extend([p[2], p[1], p[0]]);
                        if bpp == 32 {
                            data.push(0);
                        }
                    }
                    data.resize(start + stride, 0);
                }
                assert_eq!(decode_dib(&data).unwrap(), image);
            }
        }
        let mut fields = dib[..40].to_vec();
        fields[16..20].copy_from_slice(&3u32.to_le_bytes());
        for mask in [0xff0000u32, 0xff00, 0xff] {
            fields.extend(mask.to_le_bytes());
        }
        fields.extend(&dib[40..]);
        assert_eq!(decode_dib(&fields).unwrap(), image);
        assert!(decode_dib(&dib[..20]).is_err());
    }

    #[test]
    fn dibv5_preserves_orientation_colors_and_transparency() {
        let image = RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([x as u8 * 50, y as u8 * 80, 37, 120])
        });
        let dib = encode_dibv5(&image);
        assert_eq!(dib.len(), 124 + 3 * 2 * 4);
        assert_eq!(decode_dib(&dib).unwrap(), image);
    }
}
