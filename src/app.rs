//! ウィンドウと入力の処理

use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use windows::core::{w, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::decode::{Decoder, Done};
use crate::files;
use crate::gesture::{self, Command, Tracker};
use crate::panel::{self, Kind};
use crate::render::{client_size, Bitmap, PanelScene, Renderer, Scene, SliderScene};
use crate::settings::{self, Options, Settings};
use crate::slider::{self, Bar};
use crate::view::{Mode, Size, View};

const WM_APP_DECODED: u32 = WM_APP + 1;
#[cfg(feature = "snapshot")]
const WM_APP_SNAPSHOT: u32 = WM_APP + 2;
const TIMER_TOAST: usize = 1;
const TIMER_CURSOR: usize = 2;
const TOAST_MS: u32 = 1200;
/// 操作がないとカーソルを消すまでの時間（NeeView の既定と同じ 2 秒）
const CURSOR_HIDE_MS: u32 = 2000;
const MK_LBUTTON: usize = 0x0001;
const MK_RBUTTON: usize = 0x0002;
/// マウスジェスチャーで向きを読み取る距離（DIP）。NeeView の既定と同じ
const GESTURE_DIP: f32 = 30.0;
const HTCLIENT: u32 = 1;
const WHEEL_DELTA: i32 = 120;
/// 残りがこれ未満なら端に着いたとみなす（DIP）。NeeView の「スクロール終端マージン」の既定と同じ
const SCROLL_END_MARGIN_DIP: f64 = 10.0;
const EMPTY_TEXT: &str = "画像ファイルかフォルダをドロップしてください";

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct Overlay {
    text: Option<String>,
    slider: Option<SliderScene>,
    panel: Option<PanelScene>,
    /// 開けなかったページでは画像を出さない
    show_image: bool,
}

struct Scroll {
    from: (f64, f64),
    to: View,
    start: Instant,
}

enum Slot {
    Ready(Bitmap),
    Failed(String),
}

struct App {
    hwnd: HWND,
    renderer: Renderer,
    decoder: Decoder,
    files: Vec<PathBuf>,
    index: usize,
    /// 直前の移動が「次へ」だったか。先読みの向きと、はみ出す画像の初期位置に使う
    forward: bool,
    /// 表示中・先読み済みの画像。表示中のページと前後 1 枚だけを持つ
    cache: Vec<(PathBuf, Slot)>,
    /// 再読み込みを頼んだ画像。新しく読めるまでは cache の古い画像を使う
    stale: Vec<PathBuf>,
    /// 今描いている画像。ページを移っても、次が読めるまでは前の画像を出しておく
    shown: Option<PathBuf>,
    view: View,
    mode: Mode,
    client: Size,
    /// 画像を置く範囲。ウィンドウ表示ではクライアント領域からスライダーの分を除いたもの
    area: Size,
    dpi: u32,
    /// 開けなかったときに出す文
    notice: Option<String>,
    drag: Option<(i32, i32)>,
    /// スライダーを押したまま動かしている
    seeking: bool,
    /// 全画面でマウスが下端にあり、スライダーを出している
    bar_peek: bool,
    /// ホイールでのスクロール中のアニメーション
    scroll: Option<Scroll>,
    options: Options,
    /// 設定パネルを開いている
    panel: bool,
    /// 設定パネルのスライダーを押したまま動かしている（行の番号）
    panel_drag: Option<usize>,
    /// 右ボタンを押して動かしている途中のジェスチャー
    gesture: Option<Tracker>,
    wheel: i32,
    toast: Option<String>,
    cursor_hidden: bool,
    cursor_at: (i32, i32),
    /// 全画面にする前のウィンドウ位置
    fullscreen: Option<WINDOWPLACEMENT>,
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let class = w!("karui.main");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // 描画の準備ができるまで白く光らないように黒で塗る
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let settings = settings::load();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("karui"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            860,
            None,
            None,
            Some(instance),
            None,
        )
        .expect("ウィンドウを作れませんでした");
        // 表示する前に合わせておく（白いタイトルバーが一瞬見えないように）
        apply_theme(hwnd);

        let app = App::new(hwnd, settings.mode, settings.options);
        APP.with(|a| *a.borrow_mut() = Some(app));

        // ウィンドウを出す前に読み込みを始めておく
        if let Some(arg) = std::env::args_os().nth(1) {
            with_app(|app| app.open(Path::new(&arg)));
        }

        match settings.rect {
            Some([left, top, right, bottom]) => {
                let wp = WINDOWPLACEMENT {
                    length: size_of::<WINDOWPLACEMENT>() as u32,
                    showCmd: if settings.maximized { SW_SHOWMAXIMIZED.0 as u32 } else { SW_SHOWNORMAL.0 as u32 },
                    rcNormalPosition: RECT { left, top, right, bottom },
                    ..Default::default()
                };
                let _ = SetWindowPlacement(hwnd, &wp);
            }
            None => {
                let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            }
        }
        DragAcceptFiles(hwnd, true);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| a.borrow_mut().take());
    }
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Ok(mut g) = a.try_borrow_mut() {
            if let Some(app) = g.as_mut() {
                f(app);
            }
        }
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // 処理中に Windows から同期的に呼び返されたとき（SetWindowPos など）は借用できないので、
    // 既定の処理に任せる。サイズ変更は WM_PAINT 側で拾い直す。
    let r = APP.with(|a| match a.try_borrow_mut() {
        Ok(mut g) => g.as_mut().and_then(|app| app.handle(msg, wp, lp)),
        Err(_) => None,
    });
    match r {
        Some(r) => r,
        None => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

impl App {
    fn new(hwnd: HWND, mode: Mode, options: Options) -> App {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let renderer = Renderer::new(hwnd, dpi).expect("Direct2D を初期化できませんでした");
        let (w, h) = client_size(hwnd);
        App {
            hwnd,
            renderer,
            decoder: Decoder::new(hwnd, WM_APP_DECODED, 2),
            files: Vec::new(),
            index: 0,
            forward: true,
            cache: Vec::new(),
            stale: Vec::new(),
            shown: None,
            view: View::default(),
            mode,
            client: (w as f64, h as f64),
            area: (w as f64, h as f64),
            dpi,
            notice: None,
            drag: None,
            seeking: false,
            bar_peek: false,
            scroll: None,
            options,
            panel: false,
            panel_drag: None,
            gesture: None,
            wheel: 0,
            toast: None,
            cursor_hidden: false,
            cursor_at: (0, 0),
            fullscreen: None,
        }
    }

    /// 処理したら Some。None なら DefWindowProc に回す
    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe { BeginPaint(self.hwnd, &mut ps) };
                self.paint();
                unsafe {
                    let _ = EndPaint(self.hwnd, &ps);
                }
            }
            WM_ERASEBKGND => return Some(LRESULT(1)),
            WM_SIZE => {
                self.sync_layout();
                self.invalidate();
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xffff) as u16 as i16 as i32;
                self.on_wheel(delta, wp.0 & MK_LBUTTON != 0);
            }
            WM_LBUTTONDOWN => {
                let p = point(lp);
                // スクロール中ならその場で止めてからつかむ
                self.scroll = None;
                self.show_cursor();
                unsafe { SetCapture(self.hwnd) };
                if self.panel {
                    self.panel_click(p);
                    return Some(LRESULT(0));
                }
                match self.bar().and_then(|b| b.hit(p.0 as f32, p.1 as f32)) {
                    Some(slider::Hit::Gear) => self.open_panel(),
                    Some(slider::Hit::Track) => {
                        // スライダー: 押した位置のページへ。押したまま動かすと続けて移る
                        self.seeking = true;
                        self.seek(p.0);
                    }
                    None => self.drag = Some(p),
                }
            }
            WM_LBUTTONUP => {
                self.drag = None;
                self.seeking = false;
                self.panel_drag = None;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            WM_CAPTURECHANGED => {
                self.drag = None;
                self.seeking = false;
                self.panel_drag = None;
                if self.gesture.take().is_some() {
                    self.invalidate();
                }
            }
            WM_RBUTTONDOWN => {
                if !self.panel {
                    let p = point(lp);
                    self.show_cursor();
                    self.gesture = Some(Tracker::new((p.0 as f32, p.1 as f32), GESTURE_DIP * self.dpi_scale()));
                    unsafe { SetCapture(self.hwnd) };
                }
            }
            WM_RBUTTONUP => {
                // 何もしないで離したときに右クリックメニュー（WM_CONTEXTMENU）を出さないよう、ここで止める
                if let Some(g) = self.gesture.take() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    self.invalidate();
                    match gesture::command(&g.dirs) {
                        Some(Command::ToggleFullscreen) => self.toggle_fullscreen(),
                        Some(Command::Reload) => self.reload(),
                        None => {}
                    }
                }
            }
            WM_MOUSEMOVE => self.on_mouse_move(point(lp), wp.0 & MK_LBUTTON != 0, wp.0 & MK_RBUTTON != 0),
            WM_SETCURSOR => {
                if (lp.0 & 0xffff) as u32 != HTCLIENT {
                    return None;
                }
                unsafe {
                    SetCursor(if self.cursor_hidden { None } else { LoadCursorW(None, IDC_ARROW).ok() });
                }
                return Some(LRESULT(1));
            }
            WM_KEYDOWN => self.on_key(VIRTUAL_KEY(wp.0 as u16)),
            WM_TIMER => self.on_timer(wp.0),
            WM_SETTINGCHANGE => {
                // Windows の「アプリモード」を切り替えたとき
                if lp.0 != 0 && unsafe { PCWSTR(lp.0 as *const u16).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet") {
                    apply_theme(self.hwnd);
                }
                return None;
            }
            WM_DROPFILES => {
                if let Some(path) = dropped_file(HDROP(wp.0 as *mut _)) {
                    self.open(&path);
                }
            }
            WM_DPICHANGED => {
                let r = unsafe { *(lp.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                self.dpi = (wp.0 & 0xffff) as u32;
                self.renderer.set_dpi(self.dpi);
                self.sync_layout();
                self.invalidate();
            }
            WM_APP_DECODED => self.on_decoded(),
            #[cfg(feature = "snapshot")]
            WM_APP_SNAPSHOT => self.snapshot(wp.0),
            WM_CLOSE => {
                // DestroyWindow は DefWindowProc に任せる（WM_DESTROY をここで受けるため）
                self.save_settings();
                return None;
            }
            WM_DESTROY => unsafe { PostQuitMessage(0) },
            _ => return None,
        }
        Some(LRESULT(0))
    }

    // ---- ファイルとページ ----

    fn open(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        // フォルダを列挙している間に、まず開いた画像そのものの読み込みを始める
        if path.is_file() {
            self.decoder.set_queue(vec![path.clone()]);
        }
        match files::list(&path) {
            Some((list, index)) => {
                self.files = list;
                self.index = index;
                self.notice = None;
            }
            None => {
                self.files.clear();
                self.shown = None;
                self.notice = Some(format!("画像が見つかりません\n{}", path.display()));
            }
        }
        self.forward = true;
        self.sync_layout();
        self.request();
        self.refresh_current();
        self.update_title();
        self.invalidate();
    }

    fn step(&mut self, delta: isize) {
        if self.files.is_empty() {
            return;
        }
        let last = self.files.len() as isize - 1;
        let target = (self.index as isize + delta).clamp(0, last);
        if target == self.index as isize {
            self.show_toast(if delta > 0 { "最後のページです" } else { "最初のページです" });
            return;
        }
        self.go_to(target as usize, delta > 0);
    }

    fn go_to(&mut self, index: usize, forward: bool) {
        if index >= self.files.len() || index == self.index {
            return;
        }
        self.index = index;
        self.forward = forward;
        self.request();
        self.refresh_current();
        self.update_title();
        self.invalidate();
    }

    /// 表示中のページ、進む向きの次、反対側の 1 枚。この順に読む
    fn wanted(&self) -> Vec<PathBuf> {
        let n = self.files.len() as isize;
        let i = self.index as isize;
        let d = if self.forward { 1 } else { -1 };
        [i, i + d, i - d]
            .into_iter()
            .filter(|&k| 0 <= k && k < n)
            .map(|k| self.files[k as usize].clone())
            .collect()
    }

    /// 要らなくなった画像を捨てて、足りない画像の読み込みを頼む
    fn request(&mut self) {
        self.trim();
        let todo = self
            .wanted()
            .into_iter()
            .filter(|p| self.stale.contains(p) || !self.cache.iter().any(|(q, _)| q == p))
            .collect();
        self.decoder.set_queue(todo);
    }

    /// フォルダを数え直し、表示中と先読み済みの画像を読み直す。
    /// 読み直しが終わるまでは今の画像を出したままにする
    fn reload(&mut self) {
        let Some(cur) = self.files.get(self.index).cloned() else { return };
        let old_index = self.index;
        let listed = if cur.exists() {
            files::list(&cur)
        } else {
            // 表示中のファイルが消えていたら、だいたい同じ位置のページにする
            cur.parent().and_then(files::list).map(|(list, _)| {
                let i = old_index.min(list.len() - 1);
                (list, i)
            })
        };
        match listed {
            Some((list, index)) => {
                self.files = list;
                self.index = index;
            }
            None => {
                self.files.clear();
                self.shown = None;
                self.notice = Some(format!("画像が見つかりません\n{}", cur.parent().unwrap_or(&cur).display()));
            }
        }
        // 開けなかった画像はもう一度試す。読めていた画像は、新しく読めたら差し替える
        self.cache.retain(|(_, s)| matches!(s, Slot::Ready(_)));
        self.stale = self.cache.iter().map(|(p, _)| p.clone()).collect();
        self.request();
        self.refresh_current();
        self.update_title();
        self.show_toast(Command::Reload.label());
    }

    /// 表示中のページと前後 1 枚、それと今描いている画像以外を捨てる
    fn trim(&mut self) {
        let wanted = self.wanted();
        let shown = self.shown.clone();
        self.cache.retain(|(p, _)| wanted.contains(p) || shown.as_ref() == Some(p));
        let cache = &self.cache;
        self.stale.retain(|p| cache.iter().any(|(q, _)| q == p));
    }

    fn on_decoded(&mut self) {
        if let Some(n) = self.renderer.max_bitmap_size() {
            self.decoder.set_max_size(n);
        }
        let wanted = self.wanted();
        for Done { path, result } in self.decoder.take_done() {
            if !wanted.contains(&path) {
                continue;
            }
            if let Some(i) = self.stale.iter().position(|p| p == &path) {
                // 再読み込み: 古い画像と差し替える。大きさが変わっていたら表示し直す
                self.stale.remove(i);
                if let Some(pos) = self.cache.iter().position(|(p, _)| p == &path) {
                    let (_, old) = self.cache.remove(pos);
                    let same_size = match (&old, &result) {
                        (Slot::Ready(b), Ok(img)) => {
                            let (w, h) = if img.orientation >= 5 { (img.orig_h, img.orig_w) } else { (img.orig_w, img.orig_h) };
                            b.display_size() == (w as f64, h as f64)
                        }
                        _ => false,
                    };
                    if !same_size && self.shown.as_ref() == Some(&path) {
                        self.shown = None;
                    }
                }
            } else if self.cache.iter().any(|(p, _)| p == &path) {
                continue;
            }
            let slot = match result {
                Ok(img) => match self.renderer.upload(&img) {
                    Ok(b) => Slot::Ready(b),
                    Err(e) => Slot::Failed(format!("表示できません: {}", e.message())),
                },
                Err(msg) => Slot::Failed(msg),
            };
            self.cache.push((path, slot));
        }
        self.refresh_current();
    }

    /// 表示中のページが読み終わっていたら、それを表示する
    fn refresh_current(&mut self) {
        let Some(cur) = self.files.get(self.index) else { return };
        match self.cache.iter().find(|(p, _)| p == cur) {
            Some((p, Slot::Ready(b))) => {
                if self.shown.as_ref() != Some(p) {
                    self.view = View::open(self.mode, b.display_size(), self.area, self.forward);
                    self.scroll = None;
                    self.shown = Some(p.clone());
                    // 次のページが読めるまで出していた前の画像は、ここで要らなくなる
                    self.trim();
                    self.update_title();
                    self.invalidate();
                }
            }
            Some((_, Slot::Failed(_))) => {
                self.shown = None;
                self.trim();
                self.invalidate();
            }
            None => {}
        }
    }

    fn shown_bitmap(&self) -> Option<&Bitmap> {
        find_ready(&self.cache, self.shown.as_ref()?)
    }

    fn current_error(&self) -> Option<String> {
        let cur = self.files.get(self.index)?;
        self.cache.iter().find_map(|(p, s)| match s {
            Slot::Failed(msg) if p == cur => {
                let name = cur.file_name().unwrap_or_default().to_string_lossy();
                Some(format!("{name}\n{msg}"))
            }
            _ => None,
        })
    }

    // ---- 表示 ----

    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if let Some(size) = self.shown_bitmap().map(|b| b.display_size()) {
            self.view = View::open(mode, size, self.area, true);
            self.scroll = None;
        }
        self.show_toast(mode.label());
        self.update_title();
        self.invalidate();
    }

    /// 画像の上に重ねるもの
    fn overlay(&self) -> Overlay {
        let error = self.current_error();
        let center = if self.files.is_empty() {
            Some(self.notice.clone().unwrap_or_else(|| EMPTY_TEXT.to_string()))
        } else {
            error.clone()
        };
        Overlay {
            text: self.gesture.as_ref().and_then(|g| g.text()).or(self.toast.clone()).or(center),
            slider: self.bar().map(|bar| SliderScene { bar, index: self.index, count: self.files.len() }),
            panel: self.panel.then(|| PanelScene { layout: self.panel_layout(), options: self.options }),
            show_image: error.is_none(),
        }
    }

    fn paint(&mut self) {
        self.sync_layout();
        self.animate();
        let o = self.overlay();
        // self.cache を借りたまま self.renderer を使うので、メソッドを通さずフィールドで取る
        let image = match (o.show_image, &self.shown) {
            (true, Some(shown)) => find_ready(&self.cache, shown).map(|b| (b, self.view)),
            _ => None,
        };
        let area = (self.area.0 as f32, self.area.1 as f32);
        let scene = Scene { image, text: o.text.as_deref(), slider: o.slider.as_ref(), panel: o.panel.as_ref(), area };
        if !self.renderer.draw(&scene) {
            // GPU がリセットされた。画像はすべて作り直し
            self.cache.clear();
            self.stale.clear();
            self.scroll = None;
            self.shown = None;
            self.request();
            self.invalidate();
        }
    }

    /// 検証用: 環境変数 KARUI_SNAPSHOT_DIR に snap_<n>.bmp を書く
    #[cfg(feature = "snapshot")]
    fn snapshot(&mut self, n: usize) {
        let Some(dir) = std::env::var_os("KARUI_SNAPSHOT_DIR") else { return };
        self.sync_layout();
        let o = self.overlay();
        let image = match (o.show_image, &self.shown) {
            (true, Some(shown)) => find_ready(&self.cache, shown).map(|b| (b, self.view)),
            _ => None,
        };
        let area = (self.area.0 as f32, self.area.1 as f32);
        let scene = Scene { image, text: o.text.as_deref(), slider: o.slider.as_ref(), panel: o.panel.as_ref(), area };
        let path = PathBuf::from(dir).join(format!("snap_{n:02}.bmp"));
        let _ = self.renderer.snapshot(&scene, &path);
        if let Ok(ms) = self.renderer.bench(&scene, 30) {
            let _ = std::fs::write(path.with_extension("txt"), format!("{ms:.2} ms/frame"));
        }
    }

    /// ウィンドウの大きさやスライダーの有無が変わっていたら、描画先と表示位置を合わせる
    fn sync_layout(&mut self) {
        let (w, h) = client_size(self.hwnd);
        if w == 0 || h == 0 {
            return; // 最小化
        }
        let client = (w as f64, h as f64);
        if client != self.client {
            self.renderer.resize(w, h);
            self.client = client;
        }
        // 全画面では画像を狭めないよう、スライダーは画像に重ねて出す
        let docked = self.fullscreen.is_none();
        let bar_h = if docked { slider::height(self.dpi_scale()) as f64 } else { 0.0 };
        let area = (client.0, (client.1 - bar_h).max(1.0));
        if area != self.area {
            if let Some(size) = self.shown_bitmap().map(|b| b.display_size()) {
                self.view = self.view.resized(self.mode, size, self.area, area);
                self.scroll = None;
            }
            self.area = area;
            self.update_title();
        }
    }

    fn dpi_scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// 出しているスライダー。全画面ではマウスが下端にあるときだけ出す（NeeView と同じ）
    fn bar(&self) -> Option<Bar> {
        if self.fullscreen.is_some() && !self.bar_peek && !self.seeking {
            return None;
        }
        let (w, h) = (self.client.0 as f32, self.client.1 as f32);
        Some(slider::layout(w, h, self.dpi_scale(), self.files.len(), self.options.slider_ltr))
    }

    fn seek(&mut self, x: i32) {
        if self.files.is_empty() {
            return;
        }
        let Some(bar) = self.bar() else { return };
        let i = bar.index_at(x as f32, self.files.len());
        if i != self.index {
            self.go_to(i, i > self.index);
        }
    }

    fn update_title(&self) {
        let title = match self.files.get(self.index) {
            Some(p) => {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                let zoom = if self.shown.as_ref() == Some(p) {
                    format!("  {}%", (self.view.scale * 100.0).round())
                } else {
                    String::new()
                };
                format!("{name}  [{}/{}]{zoom} - karui", self.index + 1, self.files.len())
            }
            None => "karui".to_string(),
        };
        unsafe {
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(title));
        }
    }

    fn show_toast(&mut self, text: &str) {
        self.toast = Some(text.to_string());
        unsafe { SetTimer(Some(self.hwnd), TIMER_TOAST, TOAST_MS, None) };
        self.invalidate();
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    // ---- 入力 ----

    fn on_wheel(&mut self, delta: i32, left_button: bool) {
        if self.panel {
            return;
        }
        // 高分解能ホイールやタッチパッドは細かく来るので、1 ノッチ分たまったら動かす
        if self.wheel != 0 && (delta > 0) != (self.wheel > 0) {
            self.wheel = 0;
        }
        self.wheel += delta;
        let steps = self.wheel / WHEEL_DELTA;
        if steps == 0 {
            return;
        }
        self.wheel -= steps * WHEEL_DELTA;

        if left_button {
            // 左ボタンを押しながらホイール: 表示サイズの切り替え。NeeView と同じく下で順送り、上で逆送り
            let mut m = self.mode;
            for _ in 0..steps.abs() {
                m = if steps < 0 { m.next() } else { m.prev() };
            }
            self.set_mode(m);
        } else {
            // はみ出した画像はスクロールし、端まで来ていたらページを移る。ホイール上で前、下で次
            for _ in 0..steps.abs() {
                self.wheel_step(steps < 0);
            }
        }
    }

    fn wheel_step(&mut self, forward: bool) {
        let Some(cur) = self.files.get(self.index) else { return };
        let o = self.options;
        let size = match self.cache.iter().find(|(p, _)| p == cur) {
            // 読み込み中は待つ。回し続けたときに、まだ見ていないページを飛ばさないように
            None if o.wait_loading => return,
            None | Some((_, Slot::Failed(_))) => None,
            Some((_, Slot::Ready(b))) => Some(b.display_size()),
        };
        if let Some(size) = size.filter(|_| o.wheel_scroll && self.shown.as_ref() == Some(cur)) {
            let k = self.dpi_scale() as f64;
            let r = o.scroll_percent as f64 / 100.0;
            let step = (self.area.0 * r, self.area.1 * r);
            let target = self.scroll.as_ref().map_or(self.view, |s| s.to);
            if let Some(to) = target.scroll_step(size, self.area, forward, step, SCROLL_END_MARGIN_DIP * k) {
                if o.scroll_ms == 0 {
                    self.view = to;
                    self.scroll = None;
                } else {
                    self.scroll = Some(Scroll { from: (self.view.x, self.view.y), to, start: Instant::now() });
                }
                self.invalidate();
                return;
            }
        }
        self.step(if forward { 1 } else { -1 });
    }

    /// スクロールのアニメーションを進める。終わっていなければ次の描画を頼む
    fn animate(&mut self) {
        let Some(s) = &self.scroll else { return };
        let secs = self.options.scroll_ms as f64 / 1000.0;
        let t = if secs > 0.0 { (s.start.elapsed().as_secs_f64() / secs).min(1.0) } else { 1.0 };
        if t >= 1.0 {
            self.view = s.to;
            self.scroll = None;
            return;
        }
        // 始めは速く、終わりはゆっくり
        let e = 1.0 - (1.0 - t).powi(3);
        self.view.x = s.from.0 + (s.to.x - s.from.0) * e;
        self.view.y = s.from.1 + (s.to.y - s.from.1) * e;
        self.invalidate();
    }

    fn on_mouse_move(&mut self, p: (i32, i32), left_button: bool, right_button: bool) {
        if self.cursor_hidden {
            // ほんの少しの揺れでは出さない（NeeView の既定と同じ 5px）
            if (p.0 - self.cursor_at.0).abs() > 5 || (p.1 - self.cursor_at.1).abs() > 5 {
                self.show_cursor();
            }
        } else {
            self.cursor_at = p;
            unsafe { SetTimer(Some(self.hwnd), TIMER_CURSOR, CURSOR_HIDE_MS, None) };
        }

        if self.fullscreen.is_some() {
            let peek = p.1 as f64 >= self.client.1 - slider::height(self.dpi_scale()) as f64;
            if peek != self.bar_peek {
                self.bar_peek = peek;
                self.invalidate();
            }
        }

        if let Some(g) = &mut self.gesture {
            if right_button {
                if g.moved((p.0 as f32, p.1 as f32)) {
                    self.invalidate();
                }
            } else {
                self.gesture = None;
                self.invalidate();
            }
            return;
        }

        if let Some(row) = self.panel_drag {
            if left_button {
                self.panel_slide(row, p.0);
            } else {
                self.panel_drag = None;
            }
            return;
        }

        if self.seeking {
            if left_button {
                self.seek(p.0);
            } else {
                self.seeking = false;
            }
            return;
        }

        let Some(last) = self.drag else { return };
        if !left_button {
            self.drag = None;
            return;
        }
        self.drag = Some(p);
        if let Some(size) = self.shown_bitmap().map(|b| b.display_size()) {
            let before = self.view;
            self.view.pan((p.0 - last.0) as f64, (p.1 - last.1) as f64, size, self.area);
            if self.view != before {
                self.invalidate();
            }
        }
    }

    fn show_cursor(&mut self) {
        self.cursor_hidden = false;
        unsafe {
            SetCursor(LoadCursorW(None, IDC_ARROW).ok());
            SetTimer(Some(self.hwnd), TIMER_CURSOR, CURSOR_HIDE_MS, None);
        }
    }

    fn on_timer(&mut self, id: usize) {
        unsafe {
            let _ = KillTimer(Some(self.hwnd), id);
        }
        match id {
            TIMER_TOAST => {
                self.toast = None;
                self.invalidate();
            }
            TIMER_CURSOR => {
                if self.drag.is_some() || self.seeking || self.panel || self.gesture.is_some() {
                    return;
                }
                // カーソルがこのウィンドウの中にあるときだけ消す
                let mut pt = POINT::default();
                unsafe {
                    if GetCursorPos(&mut pt).is_err() || WindowFromPoint(pt) != self.hwnd {
                        return;
                    }
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                if pt.x < 0 || pt.y < 0 || pt.x as f64 >= self.client.0 || pt.y as f64 >= self.client.1 {
                    return;
                }
                self.cursor_hidden = true;
                self.cursor_at = (pt.x, pt.y);
                unsafe { SetCursor(None) };
            }
            _ => {}
        }
    }

    fn on_key(&mut self, vk: VIRTUAL_KEY) {
        if self.panel {
            if vk == VK_ESCAPE {
                self.close_panel();
            }
            return;
        }
        match vk {
            // NeeView の既定（右開き）に合わせて、← が次、→ が前
            VK_LEFT => self.step(1),
            VK_RIGHT => self.step(-1),
            VK_HOME => self.go_to(0, false),
            VK_END => self.go_to(self.files.len().saturating_sub(1), true),
            VK_F11 => self.toggle_fullscreen(),
            VK_ESCAPE if self.fullscreen.is_some() => self.toggle_fullscreen(),
            _ => {}
        }
    }

    fn toggle_fullscreen(&mut self) {
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE);
            let overlapped = WS_OVERLAPPEDWINDOW.0 as isize;
            if let Some(wp) = self.fullscreen.take() {
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, style | overlapped);
                let _ = SetWindowPlacement(self.hwnd, &wp);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                );
            } else {
                let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
                let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                if GetWindowPlacement(self.hwnd, &mut wp).is_err() || !GetMonitorInfoW(monitor, &mut mi).as_bool() {
                    return;
                }
                let r = mi.rcMonitor;
                SetWindowLongPtrW(self.hwnd, GWL_STYLE, style & !overlapped);
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOP),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
                );
                self.fullscreen = Some(wp);
            }
        }
        self.bar_peek = false;
        self.sync_layout();
        self.invalidate();
    }

    fn save_settings(&self) {
        let wp = match &self.fullscreen {
            Some(wp) => *wp,
            None => {
                let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                unsafe {
                    let _ = GetWindowPlacement(self.hwnd, &mut wp);
                }
                wp
            }
        };
        let r = wp.rcNormalPosition;
        let maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32
            || (wp.showCmd == SW_SHOWMINIMIZED.0 as u32 && (wp.flags.0 & WPF_RESTORETOMAXIMIZED.0) != 0);
        Settings { mode: self.mode, rect: Some([r.left, r.top, r.right, r.bottom]), maximized, options: self.options }.save();
    }

    // ---- 設定パネル ----

    fn panel_layout(&self) -> panel::Layout {
        panel::layout(self.client.0 as f32, self.client.1 as f32, self.dpi_scale())
    }

    fn open_panel(&mut self) {
        self.panel = true;
        self.toast = None;
        self.invalidate();
    }

    fn close_panel(&mut self) {
        self.panel = false;
        self.panel_drag = None;
        self.save_settings();
        self.invalidate();
    }

    fn panel_click(&mut self, p: (i32, i32)) {
        let l = self.panel_layout();
        match l.hit(p.0 as f32, p.1 as f32) {
            panel::Hit::Outside | panel::Hit::Close => self.close_panel(),
            panel::Hit::Inside => {}
            panel::Hit::Row(i) => {
                let item = l.rows[i].item;
                if !item.enabled(&self.options) {
                    return;
                }
                match item.kind() {
                    Kind::Toggle => {
                        let v = item.get(&self.options);
                        item.set(&mut self.options, 1.0 - v);
                        self.invalidate();
                    }
                    Kind::Slider { .. } => {
                        self.panel_drag = Some(i);
                        self.panel_slide(i, p.0);
                    }
                }
            }
        }
    }

    fn panel_slide(&mut self, row: usize, x: i32) {
        let l = self.panel_layout();
        if let Some(v) = l.slider_value(row, x as f32) {
            l.rows[row].item.set(&mut self.options, v);
            self.invalidate();
        }
    }
}

fn find_ready<'a>(cache: &'a [(PathBuf, Slot)], path: &Path) -> Option<&'a Bitmap> {
    cache.iter().find_map(|(p, s)| match s {
        Slot::Ready(b) if p == path => Some(b),
        _ => None,
    })
}

/// タイトルバーを Windows の「アプリモード」（ライト / ダーク）に合わせる
fn apply_theme(hwnd: HWND) {
    let dark = BOOL::from(!apps_use_light_theme());
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const BOOL as *const _,
            size_of::<BOOL>() as u32,
        );
        // 表示中のタイトルバーを描き直させる
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

fn apps_use_light_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    // 読めなければ従来どおりライト扱い
    r.is_err() || value != 0
}

fn point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xffff) as u16 as i16 as i32, ((lp.0 >> 16) & 0xffff) as u16 as i16 as i32)
}

fn dropped_file(hdrop: HDROP) -> Option<PathBuf> {
    unsafe {
        let len = DragQueryFileW(hdrop, 0, None) as usize;
        let mut buf = vec![0u16; len + 1];
        let got = DragQueryFileW(hdrop, 0, Some(&mut buf)) as usize;
        DragFinish(hdrop);
        (got > 0).then(|| PathBuf::from(OsString::from_wide(&buf[..got])))
    }
}
