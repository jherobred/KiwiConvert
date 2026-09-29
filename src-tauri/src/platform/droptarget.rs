//! An OLE drop target that replaces WebView2's own for KiwiConvert's windows.
//!
//! It only ever reports DROPEFFECT_COPY (or NONE), so File Explorer never treats a drop on
//! KiwiConvert as a move and the original file is never touched.

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Arc;
use windows::Win32::Foundation::{HWND, LPARAM, POINTL};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
use windows::core::BOOL;
use windows_core::implement;

/// A drag event in screen coordinates (physical pixels).
#[derive(Debug, Clone)]
pub enum DropEvt {
    Enter { paths: Vec<PathBuf>, x: i32, y: i32 },
    Over { x: i32, y: i32 },
    Leave,
    Drop { paths: Vec<PathBuf>, x: i32, y: i32 },
}

/// Receives drag events and returns whether a drop at that point would be accepted.
pub type Sink = Arc<dyn Fn(DropEvt) -> bool + Send + Sync>;

#[implement(IDropTarget)]
struct Target {
    sink: Sink,
    has_files: Cell<bool>,
}

unsafe fn file_paths(data: windows_core::Ref<'_, IDataObject>) -> Vec<PathBuf> {
    let Some(data) = data.as_ref() else { return vec![] };
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let Ok(mut medium) = (unsafe { data.GetData(&format) }) else { return vec![] };
    let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    let mut paths = Vec::with_capacity(count as usize);
    for i in 0..count {
        let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize;
        let mut buf = vec![0u16; len + 1];
        unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
        paths.push(PathBuf::from(OsString::from_wide(&buf[..len])));
    }
    unsafe { ReleaseStgMedium(&mut medium) };
    paths
}

fn effect(accept: bool) -> DROPEFFECT {
    if accept { DROPEFFECT_COPY } else { DROPEFFECT_NONE }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        pdataobj: windows_core::Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        let paths = unsafe { file_paths(pdataobj) };
        let has_files = !paths.is_empty();
        self.has_files.set(has_files);
        let accept = has_files
            && (self.sink)(DropEvt::Enter {
                paths,
                x: pt.x,
                y: pt.y,
            });
        unsafe { *pdweffect = effect(accept) };
        Ok(())
    }

    fn DragOver(
        &self,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        let accept = self.has_files.get()
            && (self.sink)(DropEvt::Over {
                x: pt.x,
                y: pt.y,
            });
        unsafe { *pdweffect = effect(accept) };
        Ok(())
    }

    fn DragLeave(&self) -> windows_core::Result<()> {
        if self.has_files.replace(false) {
            (self.sink)(DropEvt::Leave);
        }
        Ok(())
    }

    fn Drop(
        &self,
        pdataobj: windows_core::Ref<'_, IDataObject>,
        _grfkeystate: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        pdweffect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        let mut accept = false;
        if self.has_files.replace(false) {
            let paths = unsafe { file_paths(pdataobj) };
            if !paths.is_empty() {
                accept = (self.sink)(DropEvt::Drop {
                    paths,
                    x: pt.x,
                    y: pt.y,
                });
            }
        }
        unsafe { *pdweffect = effect(accept) };
        Ok(())
    }
}

/// Registers the drop target on `hwnd` and every child window in this process, replacing
/// any existing registration (WebView2 registers its own). Must run on the window's thread.
pub fn attach(hwnd: HWND, sink: Sink) {
    let target: IDropTarget = Target {
        sink,
        has_files: Cell::new(false),
    }
    .into();

    unsafe {
        let _ = RevokeDragDrop(hwnd);
        let _ = RegisterDragDrop(hwnd, &target);

        let mut children: Vec<HWND> = Vec::new();
        unsafe extern "system" fn collect(child: HWND, lparam: LPARAM) -> BOOL {
            let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
            list.push(child);
            BOOL(1)
        }
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(collect),
            LPARAM(&mut children as *mut Vec<HWND> as isize),
        );
        for child in children {
            // Windows owned by the WebView2 browser process refuse registration; that is fine,
            // OLE walks up to the nearest registered ancestor.
            let _ = RevokeDragDrop(child);
            let _ = RegisterDragDrop(child, &target);
        }
    }
}
