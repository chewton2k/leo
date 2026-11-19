use std::io::{Read, Write};

use image::ImageEncoder;
use leo_core::attachments;

use super::png;

pub(crate) const MOST_PIXELS: u64 = 4_000_000_000;
const KEPT_PIXELS: u64 = 16_000_000;
const MOST_SIDE: u32 = 65_535;
const READING_SIDE: u32 = 4_000;
const DECODE_BYTES: u64 = 256 * 1024 * 1024;
const MOST_WHOLE_PIXELS: u64 = DECODE_BYTES / 3;

struct Bytes(Vec<u8>);

impl Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > attachments::MOST_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "picture exceeds the encoded size limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn reading_copy(bytes: Vec<u8>, width: u32, height: u32) -> Option<Vec<u8>> {
    if u64::from(width) * u64::from(height) <= KEPT_PIXELS {
        return Some(bytes);
    }
    if u64::from(width) * u64::from(height) > MOST_WHOLE_PIXELS {
        return None;
    }
    let format = image::guess_format(&bytes).ok()?;
    let dimensions = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
        .into_dimensions()
        .ok()?;
    if dimensions != (width, height) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_SIDE);
    limits.max_image_height = Some(MOST_SIDE);
    limits.max_alloc = Some(DECODE_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let resized = decoded.thumbnail(READING_SIDE, READING_SIDE);
    drop(decoded);
    let mut output = Bytes(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            resized.as_bytes(),
            resized.width(),
            resized.height(),
            resized.color().into(),
        )
        .ok()?;
    Some(output.0)
}

fn plain_rows(content: &[u8], width: u32, height: u32, channels: usize) -> Option<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(content);
    let mut encoder =
        flate2::write::ZlibEncoder::new(Bytes(Vec::new()), flate2::Compression::default());
    let row_bytes = usize::try_from(width).ok()?.checked_mul(channels)?;
    let mut row = vec![0; row_bytes];
    for _ in 0..height {
        decoder.read_exact(&mut row).ok()?;
        encoder.write_all(&[0]).ok()?;
        encoder.write_all(&row).ok()?;
    }
    if decoder.read(&mut [0]).ok()? != 0 {
        return None;
    }
    let idat = encoder.finish().ok()?.0;
    Some(png(width, height, channels as u8, &idat))
}

fn unfilter(kind: u8, line: &mut [u8], prior: &[u8], pixel: usize) -> Option<()> {
    for i in 0..line.len() {
        let left = if i >= pixel { line[i - pixel] } else { 0 };
        let up = prior[i];
        let corner = if i >= pixel { prior[i - pixel] } else { 0 };
        line[i] = line[i].wrapping_add(match kind {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => {
                let (a, b, c) = (i16::from(left), i16::from(up), i16::from(corner));
                let p = a + b - c;
                let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                if pa <= pb && pa <= pc {
                    left
                } else if pb <= pc {
                    up
                } else {
                    corner
                }
            }
            _ => return None,
        });
    }
    Some(())
}

fn scaled_rows(
    content: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    filtered: bool,
) -> Option<Vec<u8>> {
    let factor = f64::from(READING_SIDE) / f64::from(width.max(height));
    let out_width = ((f64::from(width) * factor).round() as u32).max(1);
    let out_height = ((f64::from(height) * factor).round() as u32).max(1);
    let row_bytes = usize::try_from(width).ok()?.checked_mul(channels)?;
    let across = |x: u32| (u64::from(x) * u64::from(out_width) / u64::from(width)) as usize;
    let mut columns = vec![0u32; out_width as usize];
    for x in 0..width {
        columns[across(x)] += 1;
    }
    let mut decoder = flate2::read::ZlibDecoder::new(content);
    let mut line = vec![0u8; row_bytes];
    let mut prior = vec![0u8; row_bytes];
    let mut tag = [0u8];
    let mut sums = vec![0u32; out_width as usize * channels];
    let mut rows_in = 0u32;
    let mut out = vec![0u8; out_width as usize * out_height as usize * channels];
    let mut out_row = 0usize;
    let mut flush = |sums: &mut Vec<u32>, rows_in: &mut u32, out_row: usize| {
        for (ox, count) in columns.iter().enumerate() {
            let share = (count * *rows_in).max(1);
            for c in 0..channels {
                let at = ox * channels + c;
                out[(out_row * out_width as usize * channels) + at] = (sums[at] / share) as u8;
            }
        }
        sums.fill(0);
        *rows_in = 0;
    };
    for y in 0..height {
        if filtered {
            decoder.read_exact(&mut tag).ok()?;
        }
        decoder.read_exact(&mut line).ok()?;
        if filtered {
            unfilter(tag[0], &mut line, &prior, channels)?;
            prior.copy_from_slice(&line);
        }
        for (x, pixel) in line.chunks_exact(channels).enumerate() {
            let base = across(x as u32) * channels;
            for (c, value) in pixel.iter().enumerate() {
                sums[base + c] += u32::from(*value);
            }
        }
        rows_in += 1;
        let next = (u64::from(y) + 1) * u64::from(out_height) / u64::from(height);
        if next as usize > out_row {
            flush(&mut sums, &mut rows_in, out_row);
            out_row += 1;
        }
    }
    let color = if channels == 1 {
        image::ExtendedColorType::L8
    } else {
        image::ExtendedColorType::Rgb8
    };
    let mut output = Bytes(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(&out, out_width, out_height, color)
        .ok()?;
    Some(output.0)
}

fn channels_of(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<usize> {
    let space = dict.get(b"ColorSpace").ok()?;
    let space = match space {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match space {
        lopdf::Object::Name(name) => match name.as_slice() {
            b"DeviceRGB" | b"CalRGB" => Some(3),
            b"DeviceGray" | b"CalGray" => Some(1),
            _ => None,
        },
        lopdf::Object::Array(parts) => {
            let kind = parts.first()?.as_name().ok()?;
            match kind {
                b"ICCBased" => {
                    let id = parts.get(1)?.as_reference().ok()?;
                    let n = doc
                        .get_object(id)
                        .ok()?
                        .as_stream()
                        .ok()?
                        .dict
                        .get(b"N")
                        .ok()?
                        .as_i64()
                        .ok()?;
                    matches!(n, 1 | 3).then_some(n as usize)
                }
                b"CalRGB" => Some(3),
                b"CalGray" => Some(1),
                _ => None,
            }
        }
        _ => None,
    }
}

fn number(dict: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok()?.as_i64().ok()
}

pub(crate) fn figure(doc: &lopdf::Document, image: &lopdf::xobject::PdfImage) -> Option<Vec<u8>> {
    let filters = image.filters.as_deref().unwrap_or(&[]);
    let (width, height) = (
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
    );
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > MOST_PIXELS
        || width > MOST_SIDE
        || height > MOST_SIDE
    {
        return None;
    }
    if filters == ["DCTDecode"] {
        let area = u64::from(width) * u64::from(height);
        if area > MOST_WHOLE_PIXELS {
            return None;
        }
        if attachments::dimensions(image.content).is_some_and(|actual| actual != (width, height)) {
            return None;
        }
        let shown = matches!(
            image.color_space.as_deref(),
            None | Some("DeviceRGB" | "DeviceGray" | "CalRGB" | "CalGray")
        ) || channels_of(doc, image.origin_dict).is_some();
        return shown
            .then_some(())
            .and_then(|()| reading_copy(image.content.to_vec(), width, height));
    }
    if filters != ["FlateDecode"] || image.bits_per_component != Some(8) {
        return None;
    }
    let channels = channels_of(doc, image.origin_dict)?;
    let parms = match image.origin_dict.get(b"DecodeParms") {
        Ok(lopdf::Object::Dictionary(d)) => Some(d.clone()),
        Ok(lopdf::Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    };
    let predictor = parms
        .as_ref()
        .and_then(|p| number(p, b"Predictor"))
        .unwrap_or(1);
    let filtered = match predictor {
        1 => false,
        10.. => {
            let p = parms?;
            let fits = number(&p, b"Colors").unwrap_or(1) == channels as i64
                && number(&p, b"BitsPerComponent").unwrap_or(8) == 8
                && number(&p, b"Columns").unwrap_or(1) == i64::from(width);
            if !fits {
                return None;
            }
            true
        }
        _ => return None,
    };
    if u64::from(width) * u64::from(height) > KEPT_PIXELS {
        return scaled_rows(image.content, width, height, channels, filtered);
    }
    if filtered {
        return Some(png(width, height, channels as u8, image.content));
    }
    plain_rows(image.content, width, height, channels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn unsafe_pdf_images_are_refused_before_decoding() {
        let doc = lopdf::Document::with_version("1.5");
        let dict = dictionary! { "ColorSpace" => "DeviceGray" };
        for (width, height) in [
            (0, 400),
            (-1, 400),
            (i64::MAX, i64::MAX),
            (9000, 9000),
            (70000, 300),
        ] {
            let image = lopdf::xobject::PdfImage {
                id: (1, 0),
                width,
                height,
                color_space: Some("DeviceGray".into()),
                filters: Some(vec!["DCTDecode".into()]),
                bits_per_component: Some(8),
                content: &[0xff, 0xd8, 0xff, 1],
                origin_dict: &dict,
            };
            assert!(figure(&doc, &image).is_none());
        }
        let encoded = png(
            2,
            2,
            1,
            &super::super::filtered_rows(&[1; 4], 2, 2, 1).unwrap(),
        );
        assert!(reading_copy(encoded, 5000, 3600).is_none());
    }

    fn gray_scan(width: u32, height: u32, fill: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut row = vec![0u8; width as usize];
        for y in 0..height {
            for (x, cell) in row.iter_mut().enumerate() {
                *cell = fill(x as u32, y);
            }
            encoder.write_all(&row).unwrap();
        }
        encoder.finish().unwrap()
    }

    fn scan_image<'a>(
        content: &'a [u8],
        width: i64,
        height: i64,
        dict: &'a lopdf::Dictionary,
    ) -> lopdf::xobject::PdfImage<'a> {
        lopdf::xobject::PdfImage {
            id: (1, 0),
            width,
            height,
            color_space: Some("DeviceGray".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(8),
            content,
            origin_dict: dict,
        }
    }

    fn copy_of(png: &[u8]) -> image::DynamicImage {
        image::load_from_memory(png).unwrap()
    }

    #[test]
    fn a_seventy_megapixel_scan_is_read_through_a_smaller_copy() {
        let doc = lopdf::Document::with_version("1.5");
        let dict = dictionary! { "ColorSpace" => "DeviceGray" };
        let content = gray_scan(10_000, 7_000, |_, _| 200);
        let source = content.clone();
        let made = figure(&doc, &scan_image(&content, 10_000, 7_000, &dict)).unwrap();
        let copy = copy_of(&made);
        assert_eq!((copy.width(), copy.height()), (4_000, 2_800));
        assert!(u64::from(copy.width()) * u64::from(copy.height()) <= KEPT_PIXELS);
        assert_eq!(
            image::GenericImageView::get_pixel(&copy, 2_000, 1_400).0[0],
            200
        );
        assert_eq!(content, source);
    }

    #[test]
    fn rows_stored_with_every_png_filter_give_the_same_copy_as_plain_rows() {
        let (width, height) = (5_000u32, 3_400u32);
        let value = |x: u32, y: u32| (x.wrapping_mul(7) ^ y.wrapping_mul(13)) as u8;
        let plain = gray_scan(width, height, value);
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut prior = vec![0u8; width as usize];
        for y in 0..height {
            let row: Vec<u8> = (0..width).map(|x| value(x, y)).collect();
            let kind = (y % 5) as u8;
            let at = |i: usize, back: usize| if i >= back { row[i - back] } else { 0 };
            let filtered: Vec<u8> = (0..row.len())
                .map(|i| {
                    let (left, up) = (at(i, 1), prior[i]);
                    let corner = if i >= 1 { prior[i - 1] } else { 0 };
                    row[i].wrapping_sub(match kind {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                        _ => {
                            let (a, b, c) = (i16::from(left), i16::from(up), i16::from(corner));
                            let p = a + b - c;
                            let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                            if pa <= pb && pa <= pc {
                                left
                            } else if pb <= pc {
                                up
                            } else {
                                corner
                            }
                        }
                    })
                })
                .collect();
            encoder.write_all(&[kind]).unwrap();
            encoder.write_all(&filtered).unwrap();
            prior = row;
        }
        let stored = encoder.finish().unwrap();
        assert_eq!(
            scaled_rows(&stored, width, height, 1, true),
            scaled_rows(&plain, width, height, 1, false)
        );
    }

    #[test]
    #[ignore]
    fn scan_copy_measure() {
        let env = |key: &str| std::env::var(key).unwrap();
        let (width, height): (u32, u32) = (
            env("SCAN_W").parse().unwrap(),
            env("SCAN_H").parse().unwrap(),
        );
        let doc = lopdf::Document::with_version("1.5");
        let started = std::time::Instant::now();
        let made = match env("SCAN_KIND").as_str() {
            "jpeg-make" => {
                let raw = vec![160u8; width as usize * height as usize];
                let mut jpeg = Vec::new();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
                    .encode(&raw, width, height, image::ExtendedColorType::L8)
                    .unwrap();
                std::fs::write(env("SCAN_FILE"), jpeg).unwrap();
                return;
            }
            "jpeg" => {
                let content = std::fs::read(env("SCAN_FILE")).unwrap();
                let dict = dictionary! { "ColorSpace" => "DeviceGray" };
                let image = lopdf::xobject::PdfImage {
                    filters: Some(vec!["DCTDecode".into()]),
                    ..scan_image(&content, i64::from(width), i64::from(height), &dict)
                };
                figure(&doc, &image)
            }
            _ => {
                let content = gray_scan(width, height, |_, _| 200);
                let dict = dictionary! { "ColorSpace" => "DeviceGray" };
                figure(
                    &doc,
                    &scan_image(&content, i64::from(width), i64::from(height), &dict),
                )
            }
        };
        println!(
            "copy: {:?} bytes in {:?}",
            made.map(|m| m.len()),
            started.elapsed()
        );
    }

    #[test]
    fn raw_pdf_rows_must_match_the_declared_dimensions() {
        for size in [99, 100, 101] {
            let mut encoded =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoded.write_all(&vec![120; size]).unwrap();
            let made = plain_rows(&encoded.finish().unwrap(), 10, 10, 1);
            assert_eq!(made.is_some(), size == 100);
        }
    }

    #[test]
    fn encoded_pdf_image_output_has_a_hard_size_limit() {
        let mut output = Bytes(vec![0; attachments::MOST_BYTES - 1]);
        assert!(output.write_all(&[1, 2]).is_err());
        assert_eq!(output.0.len(), attachments::MOST_BYTES - 1);
        assert!(output.write_all(&[1]).is_ok());
        assert!(output.write_all(&[2]).is_err());
        assert_eq!(output.0.len(), attachments::MOST_BYTES);
    }
}
