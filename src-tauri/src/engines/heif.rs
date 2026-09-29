//! HEIF/HEIC container support.
//!
//! Reading: FFmpeg decodes the pixels; this module extracts what FFmpeg leaves behind: the
//! EXIF block, the ICC color profile, and the rotation/mirror properties.
//!
//! Writing: x265 (through FFmpeg) encodes one HEVC frame into an MP4, and this module
//! rewraps that frame as a single-image HEIC file (ISO/IEC 23008-12).

use anyhow::{Context, Result, anyhow};

/// One ISOBMFF box: type and payload range inside the buffer.
#[derive(Debug, Clone, Copy)]
struct BoxRef {
    kind: [u8; 4],
    start: usize,
    end: usize,
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

fn be_n(b: &[u8], i: usize, n: usize) -> Option<u64> {
    match n {
        0 => Some(0),
        2 => be16(b, i).map(u64::from),
        4 => be32(b, i).map(u64::from),
        8 => be64(b, i),
        _ => None,
    }
}

/// Lists the child boxes in `data[start..end]`.
fn children(data: &[u8], start: usize, end: usize) -> Vec<BoxRef> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        let Some(size32) = be32(data, pos) else { break };
        let kind: [u8; 4] = data[pos + 4..pos + 8].try_into().unwrap();
        let (header, size) = match size32 {
            1 => match be64(data, pos + 8) {
                Some(s) => (16usize, s as usize),
                None => break,
            },
            0 => (8, end - pos),
            s => (8, s as usize),
        };
        if size < header || pos + size > end {
            break;
        }
        out.push(BoxRef {
            kind,
            start: pos + header,
            end: pos + size,
        });
        pos += size;
    }
    out
}

fn find(list: &[BoxRef], kind: &[u8; 4]) -> Option<BoxRef> {
    list.iter().copied().find(|b| &b.kind == kind)
}

/// What KiwiConvert needs from a HEIF file besides its pixels.
#[derive(Debug, Default)]
pub struct HeifMeta {
    /// TIFF-structured EXIF data, without the "Exif\0\0" prefix.
    pub exif: Option<Vec<u8>>,
    pub icc: Option<Vec<u8>>,
    /// Counter-clockwise rotation in quarter turns, from the `irot` property.
    pub rotation: u8,
    /// Mirror axis from `imir`: 0 = vertical axis (left-right flip), 1 = horizontal axis.
    pub mirror: Option<u8>,
    /// True when the transforms are listed mirror-first.
    pub mirror_first: bool,
}

pub fn read_meta(data: &[u8]) -> Result<HeifMeta> {
    let top = children(data, 0, data.len());
    let meta = find(&top, b"meta").ok_or_else(|| anyhow!("no meta box"))?;
    // `meta` is a FullBox: skip version and flags.
    let items = children(data, meta.start + 4, meta.end);
    let mut out = HeifMeta::default();

    let primary = find(&items, b"pitm").and_then(|b| {
        if data[b.start] == 0 { be16(data, b.start + 4).map(u32::from) } else { be32(data, b.start + 4) }
    });

    // Item types, to find the Exif item.
    let mut exif_id = None;
    if let Some(iinf) = find(&items, b"iinf") {
        let v = data[iinf.start];
        let first = if v == 0 { iinf.start + 6 } else { iinf.start + 8 };
        for infe in children(data, first, iinf.end).iter().filter(|b| &b.kind == b"infe") {
            let ver = data[infe.start];
            let (id, type_at) = match ver {
                2 => (be16(data, infe.start + 4).map(u32::from), infe.start + 8),
                3 => (be32(data, infe.start + 4), infe.start + 10),
                _ => continue,
            };
            if data.get(type_at..type_at + 4) == Some(b"Exif") {
                exif_id = id;
            }
        }
    }

    // Item locations.
    if let (Some(id), Some(iloc)) = (exif_id, find(&items, b"iloc")) {
        let idat = find(&items, b"idat");
        if let Some(bytes) = item_bytes(data, iloc, idat, id) {
            let offset = be32(&bytes, 0).unwrap_or(0) as usize;
            let tiff = bytes.get(4 + offset..).unwrap_or_default();
            let tiff = tiff.strip_prefix(b"Exif\0\0".as_slice()).unwrap_or(tiff);
            if tiff.starts_with(b"II") || tiff.starts_with(b"MM") {
                out.exif = Some(tiff.to_vec());
            }
        }
    }

    // Properties: ICC profile and transforms associated with the primary item.
    if let Some(iprp) = find(&items, b"iprp") {
        let props = children(data, iprp.start, iprp.end);
        let ipco = find(&props, b"ipco").map(|b| children(data, b.start, b.end)).unwrap_or_default();
        let mut associated: Vec<usize> = Vec::new();
        if let (Some(ipma), Some(primary)) = (find(&props, b"ipma"), primary) {
            let ver = data[ipma.start];
            let flags = be32(data, ipma.start).unwrap_or(0) & 0xFF_FFFF;
            let mut p = ipma.start + 4;
            let count = be32(data, p).unwrap_or(0);
            p += 4;
            for _ in 0..count {
                let id = if ver < 1 {
                    let v = be16(data, p).map(u32::from);
                    p += 2;
                    v
                } else {
                    let v = be32(data, p);
                    p += 4;
                    v
                };
                let n = *data.get(p).unwrap_or(&0) as usize;
                p += 1;
                for _ in 0..n {
                    let index = if flags & 1 != 0 {
                        let v = be16(data, p).unwrap_or(0) & 0x7FFF;
                        p += 2;
                        v as usize
                    } else {
                        let v = data.get(p).copied().unwrap_or(0) & 0x7F;
                        p += 1;
                        v as usize
                    };
                    if id == Some(primary) && index > 0 {
                        associated.push(index - 1);
                    }
                }
            }
        }
        let mut seen_rotation = false;
        for i in &associated {
            let Some(b) = ipco.get(*i) else { continue };
            match &b.kind {
                b"colr" if data.get(b.start..b.start + 4) == Some(b"prof") || data.get(b.start..b.start + 4) == Some(b"rICC") => {
                    out.icc = Some(data[b.start + 4..b.end].to_vec());
                }
                b"irot" => {
                    out.rotation = data[b.start] & 0x03;
                    seen_rotation = true;
                }
                b"imir" => {
                    out.mirror = Some(data[b.start] & 0x01);
                    out.mirror_first = !seen_rotation;
                }
                _ => {}
            }
        }
        if out.icc.is_none() {
            out.icc = ipco
                .iter()
                .find(|b| &b.kind == b"colr" && data.get(b.start..b.start + 4) == Some(b"prof"))
                .map(|b| data[b.start + 4..b.end].to_vec());
        }
    }
    Ok(out)
}

/// Reads an item's bytes via `iloc` (file offsets or the `idat` box).
fn item_bytes(data: &[u8], iloc: BoxRef, idat: Option<BoxRef>, want: u32) -> Option<Vec<u8>> {
    let ver = data[iloc.start];
    let mut p = iloc.start + 4;
    let sizes = be16(data, p)?;
    p += 2;
    let offset_size = (sizes >> 12) as usize;
    let length_size = ((sizes >> 8) & 0xF) as usize;
    let base_size = ((sizes >> 4) & 0xF) as usize;
    let index_size = if ver >= 1 { (sizes & 0xF) as usize } else { 0 };
    let count = if ver < 2 {
        let c = be16(data, p)? as u32;
        p += 2;
        c
    } else {
        let c = be32(data, p)?;
        p += 4;
        c
    };
    for _ in 0..count {
        let id = if ver < 2 {
            let v = be16(data, p)? as u32;
            p += 2;
            v
        } else {
            let v = be32(data, p)?;
            p += 4;
            v
        };
        let method = if ver >= 1 {
            let m = be16(data, p)? & 0xF;
            p += 2;
            m
        } else {
            0
        };
        p += 2; // data_reference_index
        let base = be_n(data, p, base_size)?;
        p += base_size;
        let extents = be16(data, p)?;
        p += 2;
        let mut bytes = Vec::new();
        for _ in 0..extents {
            p += index_size;
            let off = be_n(data, p, offset_size)?;
            p += offset_size;
            let len = be_n(data, p, length_size)?;
            p += length_size;
            if id == want {
                let start = match method {
                    0 => (base + off) as usize,
                    1 => idat?.start + (base + off) as usize,
                    _ => return None,
                };
                let end = if len == 0 { data.len() } else { start + len as usize };
                bytes.extend_from_slice(data.get(start..end)?);
            }
        }
        if id == want {
            return Some(bytes);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

/// The encoded HEVC frame and its decoder configuration, pulled out of an MP4.
pub struct HevcFrame {
    /// The complete `hvcC` box (header included).
    pub hvcc_box: Vec<u8>,
    /// The frame as length-prefixed NAL units.
    pub sample: Vec<u8>,
}

/// Extracts the first sample and its `hvcC` configuration from an MP4 written by FFmpeg.
pub fn hevc_from_mp4(data: &[u8]) -> Result<HevcFrame> {
    let top = children(data, 0, data.len());
    let moov = find(&top, b"moov").context("no moov box")?;
    let trak = find(&children(data, moov.start, moov.end), b"trak").context("no trak box")?;
    let mdia = find(&children(data, trak.start, trak.end), b"mdia").context("no mdia box")?;
    let minf = find(&children(data, mdia.start, mdia.end), b"minf").context("no minf box")?;
    let stbl = find(&children(data, minf.start, minf.end), b"stbl").context("no stbl box")?;
    let tables = children(data, stbl.start, stbl.end);

    let stsd = find(&tables, b"stsd").context("no stsd box")?;
    // stsd: FullBox + entry count, then sample entries.
    let entries = children(data, stsd.start + 8, stsd.end);
    let entry = entries
        .iter()
        .find(|b| &b.kind == b"hvc1" || &b.kind == b"hev1")
        .context("the encoder did not produce HEVC")?;
    // VisualSampleEntry fields take 78 bytes before the child boxes.
    let hvcc = find(&children(data, entry.start + 78, entry.end), b"hvcC").context("no hvcC box")?;
    let hvcc_box = data[hvcc.start - 8..hvcc.end].to_vec();

    let stsz = find(&tables, b"stsz").context("no stsz box")?;
    let fixed = be32(data, stsz.start + 4).unwrap_or(0);
    let size = if fixed != 0 { fixed } else { be32(data, stsz.start + 12).context("empty stsz")? } as usize;
    let offset = if let Some(stco) = find(&tables, b"stco") {
        be32(data, stco.start + 8).context("empty stco")? as usize
    } else {
        let co64 = find(&tables, b"co64").context("no chunk offsets")?;
        be64(data, co64.start + 8).context("empty co64")? as usize
    };
    let sample = data
        .get(offset..offset + size)
        .context("sample outside the file")?
        .to_vec();
    Ok(HevcFrame { hvcc_box, sample })
}

fn push_box(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
}

fn full_box(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(body.len() + 4);
    payload.push(version);
    payload.extend_from_slice(&flags.to_be_bytes()[1..]);
    payload.extend_from_slice(body);
    let mut out = Vec::new();
    push_box(&mut out, kind, &payload);
    out
}

/// Builds a single-image HEIC. `width`/`height` are the coded size; `display` crops to the
/// real image size when the encoder needed even dimensions.
pub fn build_heic(frame: &HevcFrame, width: u32, height: u32, display: (u32, u32), exif: Option<&[u8]>, icc: Option<&[u8]>) -> Vec<u8> {
    let mut ftyp = Vec::new();
    push_box(&mut ftyp, b"ftyp", b"heic\0\0\0\0mif1heicmiaf");

    // Item 1 is the image; item 2, when present, is the EXIF block.
    let exif_payload = exif.map(|e| {
        let mut v = 0u32.to_be_bytes().to_vec();
        v.extend_from_slice(e);
        v
    });

    let hdlr = full_box(b"hdlr", 0, 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
    let pitm = full_box(b"pitm", 0, 0, &1u16.to_be_bytes());

    let mut infe_list = full_box(b"infe", 2, 0, &[&1u16.to_be_bytes()[..], &[0, 0], b"hvc1", b"\0"].concat());
    if exif_payload.is_some() {
        infe_list.extend(full_box(b"infe", 2, 1, &[&2u16.to_be_bytes()[..], &[0, 0], b"Exif", b"\0"].concat()));
    }
    let item_count: u16 = if exif_payload.is_some() { 2 } else { 1 };
    let iinf = full_box(b"iinf", 0, 0, &[&item_count.to_be_bytes()[..], &infe_list].concat());

    let iref = if exif_payload.is_some() {
        // The EXIF item describes ("cdsc") the image item.
        let mut cdsc = Vec::new();
        push_box(&mut cdsc, b"cdsc", &[&2u16.to_be_bytes()[..], &1u16.to_be_bytes(), &1u16.to_be_bytes()].concat());
        full_box(b"iref", 0, 0, &cdsc)
    } else {
        Vec::new()
    };

    // Properties.
    let mut ipco_body = frame.hvcc_box.clone();
    ipco_body.extend(full_box(b"ispe", 0, 0, &[width.to_be_bytes(), height.to_be_bytes()].concat()));
    let mut colr = Vec::new();
    match icc {
        Some(profile) => push_box(&mut colr, b"colr", &[b"prof".as_slice(), profile].concat()),
        // sRGB primaries and transfer, BT.601 matrix, limited range: what FFmpeg produced.
        None => push_box(&mut colr, b"colr", &[b"nclx".as_slice(), &1u16.to_be_bytes(), &13u16.to_be_bytes(), &6u16.to_be_bytes(), &[0u8]].concat()),
    }
    ipco_body.extend(colr);
    ipco_body.extend(full_box(b"pixi", 0, 0, &[3, 8, 8, 8]));
    let mut associations = vec![0x81u8, 0x02, 0x03, 0x04];
    if display != (width, height) {
        let (dw, dh) = display;
        let mut clap = Vec::new();
        for v in [dw as i32, 1, dh as i32, 1, dw as i32 - width as i32, 2, dh as i32 - height as i32, 2] {
            clap.extend_from_slice(&v.to_be_bytes());
        }
        push_box(&mut ipco_body, b"clap", &clap);
        associations.push(0x85);
    }
    let mut ipco = Vec::new();
    push_box(&mut ipco, b"ipco", &ipco_body);
    let ipma_body = [&1u32.to_be_bytes()[..], &1u16.to_be_bytes(), &[associations.len() as u8], &associations].concat();
    let ipma = full_box(b"ipma", 0, 0, &ipma_body);
    let mut iprp = Vec::new();
    push_box(&mut iprp, b"iprp", &[ipco, ipma].concat());

    // iloc needs absolute offsets into mdat, which depend on the size of meta itself.
    let iloc_for = |image_off: u32, exif_off: u32| {
        let mut body = vec![0x44, 0x00];
        body.extend_from_slice(&item_count.to_be_bytes());
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&image_off.to_be_bytes());
        body.extend_from_slice(&(frame.sample.len() as u32).to_be_bytes());
        if let Some(e) = &exif_payload {
            body.extend_from_slice(&2u16.to_be_bytes());
            body.extend_from_slice(&0u16.to_be_bytes());
            body.extend_from_slice(&1u16.to_be_bytes());
            body.extend_from_slice(&exif_off.to_be_bytes());
            body.extend_from_slice(&(e.len() as u32).to_be_bytes());
        }
        full_box(b"iloc", 0, 0, &body)
    };
    let meta_for = |iloc: Vec<u8>| full_box(b"meta", 0, 0, &[hdlr.clone(), pitm.clone(), iloc, iinf.clone(), iref.clone(), iprp.clone()].concat());

    let meta_len = meta_for(iloc_for(0, 0)).len();
    let mdat_start = (ftyp.len() + meta_len + 8) as u32;
    let exif_start = mdat_start + frame.sample.len() as u32;
    let meta = meta_for(iloc_for(mdat_start, exif_start));

    let mut mdat_payload = frame.sample.clone();
    if let Some(e) = &exif_payload {
        mdat_payload.extend_from_slice(e);
    }
    let mut out = ftyp;
    out.extend(meta);
    push_box(&mut out, b"mdat", &mdat_payload);
    out
}

#[cfg(test)]
fn looks_like_heif(data: &[u8]) -> bool {
    data.get(4..8) == Some(b"ftyp") && data.len() > 16
}

#[cfg(test)]
fn ensure_heif(data: &[u8]) -> Result<()> {
    if !looks_like_heif(data) {
        anyhow::bail!("not a HEIF file");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_frame() -> HevcFrame {
        let mut hvcc = Vec::new();
        push_box(&mut hvcc, b"hvcC", &[1, 2, 3, 4]);
        HevcFrame { hvcc_box: hvcc, sample: vec![0, 0, 0, 3, 0xAA, 0xBB, 0xCC] }
    }

    #[test]
    fn written_heic_reads_back() {
        let exif = b"MM\0*\0\0\0\x08\0\0".to_vec();
        let icc = vec![9u8; 40];
        let file = build_heic(&fake_frame(), 64, 48, (63, 47), Some(&exif), Some(&icc));
        ensure_heif(&file).unwrap();
        let meta = read_meta(&file).unwrap();
        assert_eq!(meta.exif.as_deref(), Some(exif.as_slice()));
        assert_eq!(meta.icc.as_deref(), Some(icc.as_slice()));
        assert_eq!(meta.rotation, 0);

        // The image item's iloc entry must point at the sample inside mdat.
        let top = children(&file, 0, file.len());
        let meta_box = find(&top, b"meta").unwrap();
        let items = children(&file, meta_box.start + 4, meta_box.end);
        let bytes = item_bytes(&file, find(&items, b"iloc").unwrap(), None, 1).unwrap();
        assert_eq!(bytes, vec![0, 0, 0, 3, 0xAA, 0xBB, 0xCC]);
    }
}
