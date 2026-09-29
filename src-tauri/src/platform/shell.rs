//! File Explorer integration: selecting finished files and reading shell thumbnails.

use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    ILCreateFromPathW, ILFree, IShellItemImageFactory, SHCreateItemFromParsingName,
    SHOpenFolderAndSelectItems, SIIGBF,
};
use windows::core::PCWSTR;

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// Opens File Explorer with the given files selected, one window per folder. Explorer
/// reuses a window that already shows the folder.
pub fn reveal(paths: &[PathBuf]) {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for p in paths {
        if let Some(parent) = p.parent() {
            groups.entry(parent.to_path_buf()).or_default().push(p.clone());
        }
    }
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        for (dir, files) in groups {
            let dir_w = wide(&dir);
            let folder = ILCreateFromPathW(PCWSTR(dir_w.as_ptr()));
            if folder.is_null() {
                continue;
            }
            let items: Vec<*const ITEMIDLIST> = files
                .iter()
                .map(|f| {
                    let w = wide(f);
                    ILCreateFromPathW(PCWSTR(w.as_ptr())) as *const ITEMIDLIST
                })
                .filter(|p| !p.is_null())
                .collect();
            let _ = SHOpenFolderAndSelectItems(folder, Some(&items), 0);
            for item in items {
                ILFree(Some(item));
            }
            ILFree(Some(folder));
        }
    });
}

/// The thumbnail File Explorer would show for a file, as PNG bytes. Falls back to the file's
/// icon when no thumbnail handler exists for the type.
pub fn thumbnail_png(path: &Path, size: i32) -> Option<Vec<u8>> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let w = wide(path);
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let hbmp = factory
            .GetImage(SIZE { cx: size, cy: size }, SIIGBF(0))
            .ok()?;

        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(hbmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut _ as *mut _),
        );
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight == 0 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let (width, height) = (bm.bmWidth, bm.bmHeight.abs());
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height requests top-down rows.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (width * height * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            hbmp,
            0,
            height as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines == 0 {
            return None;
        }

        // BGRA -> RGBA. Shell bitmaps without an alpha channel report zero alpha everywhere.
        let opaque = buf.chunks_exact(4).all(|px| px[3] == 0);
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            if opaque {
                px[3] = 255;
            } else if px[3] > 0 && px[3] < 255 {
                // Premultiplied to straight alpha.
                let a = px[3] as u32;
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        let img = image::RgbaImage::from_raw(width as u32, height as u32, buf)?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }
}
