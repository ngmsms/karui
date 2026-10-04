//! WIC で画像を読むワーカースレッド。
//! UI スレッドは「今ほしいファイル」の一覧を渡し、終わったら WM_APP_DECODED で知らされる。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{GENERIC_READ, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::VT_UI2;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// 読み込んだ画像。pixels は premultiplied BGRA
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// 元画像のサイズ。GPU に載らない大きさのときは縮小して読むので width/height と違うことがある
    pub orig_w: u32,
    pub orig_h: u32,
    /// EXIF の Orientation (1-8)
    pub orientation: u8,
}

pub struct Done {
    pub path: PathBuf,
    pub result: Result<Image, String>,
}

#[derive(Default)]
struct State {
    queue: VecDeque<PathBuf>,
    inflight: Vec<PathBuf>,
    done: Vec<Done>,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    hwnd: usize,
    message: u32,
    max_size: AtomicU32,
}

pub struct Decoder {
    shared: Arc<Shared>,
}

impl Decoder {
    pub fn new(hwnd: HWND, message: u32, threads: usize) -> Decoder {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
            hwnd: hwnd.0 as usize,
            message,
            // Direct3D 11 世代の GPU ならほぼ 16384。描画先ができたら実際の値で上書きする
            max_size: AtomicU32::new(16384),
        });
        for _ in 0..threads {
            let s = shared.clone();
            std::thread::spawn(move || worker(s));
        }
        Decoder { shared }
    }

    /// 読み込み待ちを paths（先頭ほど優先）で置き換える。読み込み中のものは重複させない
    pub fn set_queue(&self, paths: Vec<PathBuf>) {
        let mut st = self.shared.state.lock().unwrap();
        let queue: VecDeque<PathBuf> = paths.into_iter().filter(|p| !st.inflight.contains(p)).collect();
        st.queue = queue;
        self.shared.cv.notify_all();
    }

    pub fn take_done(&self) -> Vec<Done> {
        std::mem::take(&mut self.shared.state.lock().unwrap().done)
    }

    pub fn set_max_size(&self, n: u32) {
        self.shared.max_size.store(n, Ordering::Relaxed);
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().quit = true;
        self.shared.cv.notify_all();
    }
}

fn worker(sh: Arc<Shared>) {
    let factory: IWICImagingFactory = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        match CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) {
            Ok(f) => f,
            Err(_) => return,
        }
    };
    loop {
        let path = {
            let mut st = sh.state.lock().unwrap();
            loop {
                if st.quit {
                    return;
                }
                if let Some(p) = st.queue.pop_front() {
                    st.inflight.push(p.clone());
                    break p;
                }
                st = sh.cv.wait(st).unwrap();
            }
        };
        let result = decode(&factory, &path, sh.max_size.load(Ordering::Relaxed));
        {
            let mut st = sh.state.lock().unwrap();
            st.inflight.retain(|p| p != &path);
            st.done.push(Done { path, result });
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(sh.hwnd as *mut _)), sh.message, WPARAM(0), LPARAM(0));
        }
    }
}

fn decode(factory: &IWICImagingFactory, path: &Path, max_size: u32) -> Result<Image, String> {
    unsafe {
        let name = HSTRING::from(path.as_os_str());
        let decoder = factory
            .CreateDecoderFromFilename(&name, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(|e| describe(path, e))?;
        let frame = decoder.GetFrame(0).map_err(|e| describe(path, e))?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).map_err(|e| describe(path, e))?;
        if w == 0 || h == 0 {
            return Err("画像のサイズが 0 です".into());
        }
        let orientation = orientation(&frame);

        let converter = factory.CreateFormatConverter().map_err(|e| describe(path, e))?;
        converter
            .Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
            .map_err(|e| describe(path, e))?;
        let mut source: IWICBitmapSource = converter.into();

        // GPU のテクスチャ上限を超える画像は縮小して読む
        let (mut bw, mut bh) = (w, h);
        if w > max_size || h > max_size {
            let k = max_size as f64 / w.max(h) as f64;
            bw = ((w as f64 * k).floor() as u32).clamp(1, max_size);
            bh = ((h as f64 * k).floor() as u32).clamp(1, max_size);
            let scaler = factory.CreateBitmapScaler().map_err(|e| describe(path, e))?;
            scaler.Initialize(&source, bw, bh, WICBitmapInterpolationModeFant).map_err(|e| describe(path, e))?;
            source = scaler.into();
        }

        let stride = bw as usize * 4;
        let mut pixels = vec![0u8; stride * bh as usize];
        source
            .CopyPixels(std::ptr::null(), stride as u32, &mut pixels)
            .map_err(|e| describe(path, e))?;

        Ok(Image { width: bw, height: bh, pixels, orig_w: w, orig_h: h, orientation })
    }
}

/// EXIF の向き。読めなければ 1（そのまま）
fn orientation(frame: &IWICBitmapFrameDecode) -> u8 {
    unsafe {
        let Ok(reader) = frame.GetMetadataQueryReader() else { return 1 };
        let mut value = PROPVARIANT::default();
        if reader.GetMetadataByName(w!("System.Photo.Orientation"), &mut value).is_err() {
            return 1;
        }
        let v = &value.Anonymous.Anonymous;
        let o = if v.vt == VT_UI2 { v.Anonymous.uiVal } else { 1 };
        let _ = PropVariantClear(&mut value);
        if (1..=8).contains(&o) {
            o as u8
        } else {
            1
        }
    }
}

fn describe(path: &Path, e: windows::core::Error) -> String {
    const COMPONENT_NOT_FOUND: u32 = 0x88982F50;
    const UNKNOWN_FORMAT: u32 = 0x88982F07;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match e.code().0 as u32 {
        COMPONENT_NOT_FOUND | UNKNOWN_FORMAT => {
            let hint = match ext.as_str() {
                "heic" | "heif" => "\nMicrosoft Store の「HEIF 画像拡張機能」を入れると読めます",
                "avif" => "\nMicrosoft Store の「AV1 Video Extension」を入れると読めます",
                "webp" => "\nMicrosoft Store の「WebP 画像拡張機能」を入れると読めます",
                "jxl" => "\nMicrosoft Store の「JPEG XL 画像拡張機能」を入れると読めます",
                _ => "",
            };
            format!("この形式は読めません{hint}")
        }
        _ => e.message().to_string(),
    }
}
