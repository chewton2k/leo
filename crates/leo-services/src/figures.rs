mod pdf;

pub(crate) use pdf::{figure as pdf_figure, MOST_PIXELS as MOST_PDF_PIXELS};

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use anyhow::Result;
use leo_core::attachments;

use crate::import::office::joined;
use crate::import::{ImageFile, Scratch};

const LEAST_WIDE: u32 = 150;
const LEAST_TALL: u32 = 100;
const LEAST_AREA: u64 = 40_000;
const LEAST_BYTES: usize = 3_000;
const MOST_STRETCH: u32 = 6;
const REPEATED_FROM: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub mime: String,
    pub path: PathBuf,
    pub place: String,
    pub photo: bool,
}

type Stored = Option<(PathBuf, String)>;

fn worth_showing(bytes: &[u8]) -> bool {
    let Some((w, h)) = attachments::dimensions(bytes) else {
        return false;
    };
    let (long, short) = (w.max(h), w.min(h).max(1));
    w.max(h) >= LEAST_WIDE
        && w.min(h) >= LEAST_TALL
        && u64::from(w) * u64::from(h) >= LEAST_AREA
        && bytes.len() >= LEAST_BYTES
        && long / short <= MOST_STRETCH
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn keep(found: Vec<Figure>) -> Vec<Figure> {
    let mut facts: HashMap<PathBuf, (u64, bool)> = HashMap::new();
    for figure in &found {
        facts.entry(figure.path.clone()).or_insert_with(|| {
            std::fs::read(&figure.path)
                .map(|bytes| (fnv(&bytes), worth_showing(&bytes)))
                .unwrap_or_default()
        });
    }
    let mut uses: HashMap<u64, usize> = HashMap::new();
    for figure in &found {
        *uses.entry(facts[&figure.path].0).or_default() += 1;
    }
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter(|f| {
            let (hash, worth) = facts[&f.path];
            f.photo || (uses[&hash] < REPEATED_FROM && seen.insert(hash) && worth)
        })
        .collect()
}

fn store(scratch: &Scratch, bytes: &[u8]) -> Result<Stored> {
    let Some(kind) = attachments::kind_of(bytes) else {
        return Ok(None);
    };
    let Some(mime) = attachments::mime_of(kind) else {
        return Ok(None);
    };
    Ok(Some((scratch.keep_figure(kind, bytes)?, mime.to_string())))
}

fn zip_bytes(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    if name.is_empty() {
        return None;
    }
    let file = archive.by_name(name).ok()?;
    let limit = attachments::MOST_BYTES;
    if file.size() > limit as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > limit {
        return None;
    }
    Some(bytes)
}

fn zip_text(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    String::from_utf8(zip_bytes(archive, name)?).ok()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let rest = &tag[at..];
    Some(&rest[..rest.find('"')?])
}

fn image_links(rels: &str) -> Vec<(String, String)> {
    rels.split("<Relationship ")
        .skip(1)
        .filter_map(|tag| {
            let tag = &format!(" {}", &tag[..tag.find('>')?]);
            let kind = attribute(tag, "Type")?;
            if !kind.ends_with("/image") || attribute(tag, "TargetMode") == Some("External") {
                return None;
            }
            Some((
                attribute(tag, "Id")?.to_string(),
                attribute(tag, "Target")?.to_string(),
            ))
        })
        .collect()
}

struct Written<'a> {
    scratch: &'a Scratch,
    files: HashMap<String, Stored>,
}

fn embedded_in(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    xml: &str,
    rels: &str,
    folder: &str,
    place: &str,
    written: &mut Written<'_>,
) -> Result<Vec<Figure>> {
    let mut used: Vec<(usize, String)> = image_links(rels)
        .into_iter()
        .filter_map(|(id, target)| {
            let path = joined(folder, &target).unwrap_or_default();
            if path.is_empty() {
                return None;
            }
            let at = xml
                .find(&format!("r:embed=\"{id}\""))
                .or_else(|| xml.find(&format!("r:id=\"{id}\"")))?;
            Some((at, path))
        })
        .collect();
    used.sort();
    used.dedup_by(|a, b| a.1 == b.1);
    let mut out = Vec::new();
    for (_, path) in used {
        if !written.files.contains_key(&path) {
            let stored = match zip_bytes(archive, &path) {
                Some(bytes) => store(written.scratch, &bytes)?,
                None => None,
            };
            written.files.insert(path.clone(), stored);
        }
        if let Some((file, mime)) = &written.files[&path] {
            out.push(Figure {
                mime: mime.clone(),
                path: file.clone(),
                place: place.to_string(),
                photo: false,
            });
        }
    }
    Ok(out)
}

pub fn from_pptx(bytes: &[u8], scratch: &Scratch) -> Result<Vec<Figure>> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Ok(Vec::new());
    };
    let Ok(slides) = crate::import::office::slide_order(bytes) else {
        return Ok(Vec::new());
    };
    let legacy = !archive
        .file_names()
        .any(|name| name.is_ok_and(|name| name == "ppt/presentation.xml"));
    let mut written = Written {
        scratch,
        files: HashMap::new(),
    };
    let mut out = Vec::new();
    for (index, name) in slides.into_iter().enumerate() {
        let n = if legacy {
            name.strip_prefix("ppt/slides/slide")
                .and_then(|s| s.strip_suffix(".xml"))
                .and_then(|s| s.parse().ok())
                .unwrap_or(index + 1)
        } else {
            index + 1
        };
        let file = name.rsplit('/').next().unwrap_or_default();
        let rels_name = format!("ppt/slides/_rels/{file}.rels");
        let (Some(xml), Some(rels)) = (
            zip_text(&mut archive, &name),
            zip_text(&mut archive, &rels_name),
        ) else {
            continue;
        };
        out.extend(embedded_in(
            &mut archive,
            &xml,
            &rels,
            "ppt/slides",
            &format!("slide {n}"),
            &mut written,
        )?);
    }
    Ok(keep(out))
}

pub fn from_docx(bytes: &[u8], scratch: &Scratch) -> Result<Vec<Figure>> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Ok(Vec::new());
    };
    let (Some(xml), Some(rels)) = (
        zip_text(&mut archive, "word/document.xml"),
        zip_text(&mut archive, "word/_rels/document.xml.rels"),
    ) else {
        return Ok(Vec::new());
    };
    let mut written = Written {
        scratch,
        files: HashMap::new(),
    };
    Ok(keep(embedded_in(
        &mut archive,
        &xml,
        &rels,
        "word",
        "the document",
        &mut written,
    )?))
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

pub fn png(width: u32, height: u32, channels: u8, idat: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, if channels == 3 { 2 } else { 0 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
pub fn filtered_rows(raw: &[u8], width: usize, height: usize, channels: usize) -> Option<Vec<u8>> {
    use std::io::Write as _;
    let row = width.checked_mul(channels)?;
    if raw.len() < row.checked_mul(height)? {
        return None;
    }
    let mut rows = Vec::with_capacity((row + 1) * height);
    for line in raw.chunks_exact(row).take(height) {
        rows.push(0);
        rows.extend_from_slice(line);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&rows).ok()?;
    encoder.finish().ok()
}

pub(crate) fn from_pdf_prepared(
    doc: &lopdf::Document,
    prepared: &HashMap<lopdf::ObjectId, ImageFile>,
    scratch: &Scratch,
) -> Result<Vec<Figure>> {
    let mut known: HashMap<lopdf::ObjectId, Stored> = HashMap::new();
    let mut out = Vec::new();
    for (number, page) in doc.get_pages() {
        for image in doc.get_page_images(page).unwrap_or_default() {
            if image.width < i64::from(LEAST_TALL) || image.height < i64::from(LEAST_TALL) {
                continue;
            }
            let stored = match known.entry(image.id) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(match prepared.get(&image.id) {
                        Some(file) => Some((file.path().to_path_buf(), file.mime.clone())),
                        None => match pdf_figure(doc, &image) {
                            Some(bytes) => store(scratch, &bytes)?,
                            None => None,
                        },
                    })
                }
            };
            if let Some((path, mime)) = stored {
                out.push(Figure {
                    mime: mime.clone(),
                    path: path.clone(),
                    place: format!("page {number}"),
                    photo: false,
                });
            }
        }
    }
    Ok(keep(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use std::io::Write as _;

    fn picture(width: u32, height: u32, shade: u8) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&[shade, (x % 251) as u8, (y % 241) as u8]);
            }
        }
        let idat = filtered_rows(&raw, width as usize, height as usize, 3).unwrap();
        png(width, height, 3, &idat)
    }

    fn figure(scratch: &Scratch, bytes: &[u8], place: &str) -> Figure {
        Figure {
            mime: "image/png".into(),
            path: scratch.keep_figure("png", bytes).unwrap(),
            place: place.into(),
            photo: false,
        }
    }

    fn read(figure: &Figure) -> Vec<u8> {
        std::fs::read(&figure.path).unwrap()
    }

    #[test]
    fn a_made_png_is_a_real_png() {
        let made = picture(320, 200, 9);
        assert_eq!(attachments::kind_of(&made), Some("png"));
        assert_eq!(attachments::dimensions(&made), Some((320, 200)));
        let crc_end = made.len() - 4;
        let mut check = crc32fast::Hasher::new();
        check.update(b"IEND");
        assert_eq!(made[crc_end..], check.finalize().to_be_bytes());
        assert!(filtered_rows(&[1, 2], 4, 4, 3).is_none());
    }

    #[test]
    fn logos_icons_strips_and_repeats_are_left_out() {
        let diagram = picture(400, 300, 1);
        let other = picture(500, 260, 2);
        let logo = picture(320, 200, 3);
        let scratch = Scratch::default();
        let found = vec![
            figure(&scratch, &diagram, "slide 1"),
            figure(&scratch, &logo, "slide 1"),
            figure(&scratch, &picture(40, 40, 4), "slide 2"),
            figure(&scratch, &picture(1200, 120, 5), "slide 2"),
            figure(&scratch, &logo, "slide 2"),
            figure(&scratch, &diagram, "slide 3"),
            figure(&scratch, &logo, "slide 3"),
            figure(&scratch, &other, "slide 4"),
        ];
        let kept = keep(found);
        let places: Vec<&str> = kept.iter().map(|f| f.place.as_str()).collect();
        assert_eq!(places, ["slide 1", "slide 4"]);
        assert_eq!(read(&kept[0]), diagram);
        assert_eq!(read(&kept[1]), other);
    }

    #[test]
    fn slide_pictures_are_found_through_the_slide_and_masters_are_skipped() {
        let diagram = picture(400, 300, 7);
        let master = picture(420, 300, 8);
        let mut zipped = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
            let opts = zip::write::SimpleFileOptions::default();
            let mut add = |name: &str, bytes: &[u8]| {
                zip.start_file(name, opts).unwrap();
                zip.write_all(bytes).unwrap();
            };
            add(
                "ppt/slides/slide1.xml",
                b"<p:sld><a:t>Heaps</a:t><a:blip r:embed=\"rId2\"/></p:sld>",
            );
            add(
                "ppt/slides/_rels/slide1.xml.rels",
                b"<Relationships><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image1.png\"/><Relationship Id=\"rId3\" Type=\"http://x/image\" Target=\"../media/unused.png\"/><Relationship Id=\"rId4\" Type=\"http://x/image\" Target=\"https://example.com/a.png\" TargetMode=\"External\"/></Relationships>",
            );
            add("ppt/media/image1.png", &diagram);
            add("ppt/media/unused.png", &master);
            add("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>");
            zip.finish().unwrap();
        }
        let scratch = Scratch::default();
        let found = from_pptx(&zipped, &scratch).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].place, "slide 1");
        assert_eq!(read(&found[0]), diagram);
        assert!(from_pptx(b"not a zip", &scratch).unwrap().is_empty());
        assert_eq!(
            joined("ppt/slides", "../media/a.png").unwrap(),
            "ppt/media/a.png"
        );
        assert_eq!(joined("word", "media/a.png").unwrap(), "word/media/a.png");
        assert!(joined("word", "../../etc/passwd").is_err());
    }

    #[test]
    fn a_pdf_figure_comes_out_as_a_picture() {
        let mut doc = lopdf::Document::with_version("1.5");
        let width = 300u32;
        let height = 200u32;
        let mut seed = 7u32;
        let raw: Vec<u8> = (0..width * height * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as u8
            })
            .collect();
        let mut deflated =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        deflated.write_all(&raw).unwrap();
        let mut dict = lopdf::Dictionary::new();
        dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", lopdf::Object::Name(b"Image".to_vec()));
        dict.set("Width", i64::from(width));
        dict.set("Height", i64::from(height));
        dict.set("ColorSpace", lopdf::Object::Name(b"DeviceRGB".to_vec()));
        dict.set("BitsPerComponent", 8);
        dict.set("Filter", lopdf::Object::Name(b"FlateDecode".to_vec()));
        let image = doc.add_object(lopdf::Stream::new(dict, deflated.finish().unwrap()));
        let pages = doc.new_object_id();
        let mut xobjects = lopdf::Dictionary::new();
        xobjects.set("Im1", image);
        let mut resources = lopdf::Dictionary::new();
        resources.set("XObject", xobjects);
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let scratch = Scratch::default();
        let found = from_pdf_prepared(&doc, &HashMap::new(), &scratch).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].place, "page 1");
        assert_eq!(
            attachments::dimensions(&read(&found[0])),
            Some((width, height))
        );
        assert_eq!(found[0].mime, "image/png");
    }

    #[test]
    fn reordered_slides_keep_pictures_on_their_visible_slide_numbers() {
        let first = picture(400, 300, 1);
        let second = picture(400, 300, 2);
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, bytes: &[u8]| {
            writer.start_file(name, opts).unwrap();
            writer.write_all(bytes).unwrap();
        };
        add("ppt/presentation.xml", br#"<p:presentation xmlns:p="p" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="r2"/><p:sldId id="257" r:id="r1"/></p:sldIdLst></p:presentation>"#);
        add("ppt/_rels/presentation.xml.rels", br#"<Relationships><Relationship Id="r1" Type="http://x/slide" Target="slides/slide1.xml"/><Relationship Id="r2" Type="http://x/slide" Target="slides/slide2.xml"/></Relationships>"#);
        for number in [1, 2] {
            add(
                &format!("ppt/slides/slide{number}.xml"),
                br#"<p:sld><a:blip r:embed="image"/></p:sld>"#,
            );
            add(&format!("ppt/slides/_rels/slide{number}.xml.rels"), format!(r#"<Relationships><Relationship Id="image" Type="http://x/image" Target="../media/image{number}.png"/></Relationships>"#).as_bytes());
        }
        add("ppt/media/image1.png", &first);
        add("ppt/media/image2.png", &second);
        let bytes = writer.finish().unwrap().into_inner();
        let scratch = Scratch::default();
        let figures = from_pptx(&bytes, &scratch).unwrap();
        assert_eq!(figures.len(), 2);
        assert_eq!(
            (figures[0].place.as_str(), figures[1].place.as_str()),
            ("slide 1", "slide 2")
        );
        assert_eq!(read(&figures[0]), second);
        assert_eq!(read(&figures[1]), first);
    }

    #[test]
    fn a_pdf_figure_stored_with_png_predictors_is_rewrapped_as_is() {
        let doc = lopdf::Document::with_version("1.5");
        let rows = filtered_rows(&[90; 200 * 120], 200, 120, 1).unwrap();
        let parms = dictionary! { "Predictor" => 15, "Colors" => 1, "BitsPerComponent" => 8, "Columns" => 200 };
        let dict = dictionary! { "ColorSpace" => "DeviceGray", "DecodeParms" => parms };
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            width: 200,
            height: 120,
            color_space: Some("DeviceGray".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(8),
            content: &rows,
            origin_dict: &dict,
        };
        let made = pdf_figure(&doc, &image).unwrap();
        assert_eq!(made, png(200, 120, 1, &rows));
        let cmyk = dictionary! { "ColorSpace" => "DeviceCMYK" };
        let jpeg = lopdf::xobject::PdfImage {
            color_space: Some("DeviceCMYK".into()),
            filters: Some(vec!["DCTDecode".into()]),
            origin_dict: &cmyk,
            ..image
        };
        assert!(
            pdf_figure(&doc, &jpeg).is_none(),
            "CMYK pictures look wrong in a browser"
        );
    }

    #[test]
    fn every_figure_past_twenty_four_is_kept() {
        let scratch = Scratch::default();
        let found = from_pptx(&deck(400, false), &scratch).unwrap();
        assert_eq!(found.len(), 400);
        assert_eq!(found[399].place, "slide 400");
    }

    #[test]
    fn a_logo_on_every_slide_is_still_left_out() {
        let scratch = Scratch::default();
        let found = from_pptx(&deck(30, true), &scratch).unwrap();
        let logo = random_gray(320, 200, 99_999);
        assert_eq!(found.len(), 30);
        assert!(found.iter().all(|f| read(f) != logo));
    }

    fn random_gray(width: u32, height: u32, seed: u32) -> Vec<u8> {
        let mut state = (seed << 1) | 1;
        let raw: Vec<u8> = (0..width * height)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let idat = filtered_rows(&raw, width as usize, height as usize, 1).unwrap();
        png(width, height, 1, &idat)
    }

    fn deck(slides: usize, logo: bool) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let mut add = |name: String, bytes: &[u8]| {
            writer.start_file(name, stored).unwrap();
            writer.write_all(bytes).unwrap();
        };
        if logo {
            add("ppt/media/logo.png".into(), &random_gray(320, 200, 99_999));
        }
        for n in 1..=slides {
            let own = "<a:blip r:embed=\"rId2\"/>";
            let shared = if logo {
                "<a:blip r:embed=\"rId3\"/>"
            } else {
                ""
            };
            add(
                format!("ppt/slides/slide{n}.xml"),
                format!("<p:sld>{own}{shared}</p:sld>").as_bytes(),
            );
            let link = if logo {
                "<Relationship Id=\"rId3\" Type=\"http://x/image\" Target=\"../media/logo.png\"/>"
            } else {
                ""
            };
            add(
                format!("ppt/slides/_rels/slide{n}.xml.rels"),
                format!("<Relationships><Relationship Id=\"rId2\" Type=\"http://x/image\" Target=\"../media/image{n}.png\"/>{link}</Relationships>").as_bytes(),
            );
            add(
                format!("ppt/media/image{n}.png"),
                &random_gray(300, 200, n as u32),
            );
        }
        writer.finish().unwrap().into_inner()
    }

    fn pdf_file(path: &std::path::Path, pages: usize) {
        let mut doc = lopdf::Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for n in 0..pages {
            let mut raw = vec![0u8; 200 * 200];
            let mut state = ((n as u32) << 1) | 1;
            for byte in raw.iter_mut() {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            let mut dict = lopdf::Dictionary::new();
            dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
            dict.set("Subtype", lopdf::Object::Name(b"Image".to_vec()));
            dict.set("Width", 200);
            dict.set("Height", 200);
            dict.set("ColorSpace", lopdf::Object::Name(b"DeviceGray".to_vec()));
            dict.set("BitsPerComponent", 8);
            dict.set("Filter", lopdf::Object::Name(b"FlateDecode".to_vec()));
            let mut deflated =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            deflated.write_all(&raw).unwrap();
            let image = doc.add_object(lopdf::Stream::new(dict, deflated.finish().unwrap()));
            let mut xobjects = lopdf::Dictionary::new();
            xobjects.set("Im1", image);
            let mut resources = lopdf::Dictionary::new();
            resources.set("XObject", xobjects);
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            });
            kids.push(lopdf::Object::from(page));
        }
        doc.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => pages as i64,
                "Kids" => kids,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(path).unwrap();
    }

    fn resident_kb() -> u64 {
        leo_core::children::quiet_command("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|out| String::from_utf8_lossy(&out.stdout).trim().parse().ok())
            .unwrap_or(0)
    }

    fn found_in(bytes: &[u8], kind: &str) -> usize {
        match kind {
            "pptx" => from_pptx(bytes, &Scratch::default()).unwrap().len(),
            _ => {
                let doc = lopdf::Document::load_mem(bytes).unwrap();
                from_pdf_prepared(&doc, &HashMap::new(), &Scratch::default())
                    .unwrap()
                    .len()
            }
        }
    }

    #[test]
    #[ignore]
    fn benchmark_many_figures() {
        if let Ok(file) = std::env::var("LEO_BENCH_FIGURES_FILE") {
            let kind = std::env::var("LEO_BENCH_FIGURES_KIND").unwrap();
            let bytes = std::fs::read(&file).unwrap();
            println!("START {}", resident_kb());
            let started = std::time::Instant::now();
            let kept = found_in(&bytes, &kind);
            println!(
                "RESULT {} MB file: {kept} figures kept in {:.2} s",
                bytes.len() / 1_048_576,
                started.elapsed().as_secs_f64()
            );
            return;
        }
        let cases = std::env::var("LEO_BENCH_CASES")
            .unwrap_or_else(|_| "pptx:300,pptx:2000,pdf:1000".into());
        let dir = tempfile::tempdir().unwrap();
        for case in cases.split(',') {
            let (kind, size) = case.split_once(':').unwrap();
            let size: usize = size.parse().unwrap();
            let file = dir.path().join(format!("many.{kind}"));
            if kind == "pptx" {
                std::fs::write(&file, deck(size, false)).unwrap();
            } else {
                pdf_file(&file, size);
            }
            let out = leo_core::children::quiet_command("/usr/bin/time")
                .arg("-l")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "--nocapture",
                    "figures::tests::benchmark_many_figures",
                ])
                .env("LEO_BENCH_FIGURES_FILE", &file)
                .env("LEO_BENCH_FIGURES_KIND", kind)
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            let line = |from: &str, prefix: &str| -> String {
                from.lines()
                    .find(|l| l.trim_start().starts_with(prefix))
                    .unwrap_or("")
                    .trim()
                    .to_string()
            };
            let peak: u64 = String::from_utf8_lossy(&out.stderr)
                .lines()
                .find(|l| l.contains("maximum resident set size"))
                .and_then(|l| l.split_whitespace().next()?.parse().ok())
                .unwrap_or(0);
            let start: u64 = line(&text, "START")
                .trim_start_matches("START")
                .trim()
                .parse()
                .unwrap_or(0);
            println!(
                "{case}: {}, peak {} MB beyond the file",
                line(&text, "RESULT"),
                (peak / 1024).saturating_sub(start) / 1024
            );
        }
    }
}
