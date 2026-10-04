//! Direct2D での描画。DPI は 96 固定で作るので、座標はそのまま物理ピクセル。

use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows_numerics::{Matrix3x2, Vector2};

use crate::decode::Image;
use crate::panel::{self, Kind};
use crate::settings::Options;
use crate::slider::{self, Bar};
use crate::view::View;

const BACKGROUND: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
const TEXT: D2D1_COLOR_F = D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
const PANEL: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.7 };
const FONT_SIZE: f32 = 20.0;
const SLIDER_BG: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.13, g: 0.13, b: 0.13, a: 1.0 };
const SLIDER_TRACK: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.33, g: 0.33, b: 0.33, a: 1.0 };
const SLIDER_FILL: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.30, g: 0.56, b: 0.92, a: 1.0 };
const SLIDER_THUMB: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.92, g: 0.92, b: 0.92, a: 1.0 };
const SLIDER_LABEL: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.85, g: 0.85, b: 0.85, a: 1.0 };
const DIM: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.45 };
const PANEL_BG: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.16, g: 0.16, b: 0.16, a: 0.98 };
const PANEL_LINE: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.24, g: 0.24, b: 0.24, a: 1.0 };
const TEXT_MAIN: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.92, g: 0.92, b: 0.92, a: 1.0 };
const TEXT_DIM: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.5, g: 0.5, b: 0.5, a: 1.0 };
const TOGGLE_OFF: D2D1_COLOR_F = D2D1_COLOR_F { r: 0.36, g: 0.36, b: 0.36, a: 1.0 };
/// Segoe MDL2 Assets の歯車と ×
const GLYPH_GEAR: &str = "\u{E713}";
const GLYPH_CLOSE: &str = "\u{E711}";

/// GPU に載せた画像
pub struct Bitmap {
    bmp: ID2D1Bitmap,
    geo: Geometry,
}

impl Bitmap {
    /// EXIF の向きを反映した元画像のサイズ
    pub fn display_size(&self) -> (f64, f64) {
        self.geo.display_size()
    }
}

/// ビットマップの大きさと、元画像との関係
struct Geometry {
    w: u32,
    h: u32,
    orig_w: u32,
    orig_h: u32,
    orientation: u8,
}

impl Geometry {
    fn display_size(&self) -> (f64, f64) {
        if self.orientation >= 5 {
            (self.orig_h as f64, self.orig_w as f64)
        } else {
            (self.orig_w as f64, self.orig_h as f64)
        }
    }

    /// ビットマップの座標 → 画面座標
    fn transform(&self, view: &View) -> Affine {
        let (ow, oh) = (self.orig_w as f64, self.orig_h as f64);
        // 縮小して読んだぶんを戻す
        let unshrink = Affine::scale(ow / self.w as f64, oh / self.h as f64);
        // EXIF の向き。元画像の (x, y) が表示上どこへ行くか
        let orient = match self.orientation {
            2 => Affine([-1.0, 0.0, 0.0, 1.0, ow, 0.0]),
            3 => Affine([-1.0, 0.0, 0.0, -1.0, ow, oh]),
            4 => Affine([1.0, 0.0, 0.0, -1.0, 0.0, oh]),
            5 => Affine([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
            6 => Affine([0.0, 1.0, -1.0, 0.0, oh, 0.0]),
            7 => Affine([0.0, -1.0, -1.0, 0.0, oh, ow]),
            8 => Affine([0.0, -1.0, 1.0, 0.0, 0.0, ow]),
            _ => Affine::scale(1.0, 1.0),
        };
        // 整数位置に置いて、等倍のときににじまないようにする
        let place = Affine([view.scale, 0.0, 0.0, view.scale, view.x.round(), view.y.round()]);
        unshrink.then(&orient).then(&place)
    }
}

/// 行ベクトル形式の 2D アフィン変換 [m11, m12, m21, m22, dx, dy]（Direct2D と同じ並び）
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine([f64; 6]);

impl Affine {
    fn scale(sx: f64, sy: f64) -> Affine {
        Affine([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// self を適用してから next を適用する変換
    fn then(&self, next: &Affine) -> Affine {
        let [a11, a12, a21, a22, ax, ay] = self.0;
        let [b11, b12, b21, b22, bx, by] = next.0;
        Affine([
            a11 * b11 + a12 * b21,
            a11 * b12 + a12 * b22,
            a21 * b11 + a22 * b21,
            a21 * b12 + a22 * b22,
            ax * b11 + ay * b21 + bx,
            ax * b12 + ay * b22 + by,
        ])
    }

    fn is_identity_scale(&self) -> bool {
        let [m11, m12, m21, m22, dx, dy] = self.0;
        m11 == 1.0 && m12 == 0.0 && m21 == 0.0 && m22 == 1.0 && dx.fract() == 0.0 && dy.fract() == 0.0
    }

    fn to_d2d(self) -> Matrix3x2 {
        let [m11, m12, m21, m22, dx, dy] = self.0.map(|v| v as f32);
        Matrix3x2 { M11: m11, M12: m12, M21: m21, M22: m22, M31: dx, M32: dy }
    }
}

pub struct Scene<'a> {
    pub image: Option<(&'a Bitmap, View)>,
    pub text: Option<&'a str>,
    pub slider: Option<&'a SliderScene>,
    pub panel: Option<&'a PanelScene>,
    /// 画像を置く範囲。中央に出す文字はこの中央に置く
    pub area: (f32, f32),
}

pub struct SliderScene {
    pub bar: Bar,
    pub index: usize,
    /// 0 なら画像を開いていない（つまみとページ番号を出さない）
    pub count: usize,
}

pub struct PanelScene {
    pub layout: panel::Layout,
    pub options: Options,
}

struct Target {
    rt: ID2D1HwndRenderTarget,
    dc: ID2D1DeviceContext,
    text: ID2D1SolidColorBrush,
    panel: ID2D1SolidColorBrush,
    /// 色を変えながら使う
    ui: ID2D1SolidColorBrush,
}

pub struct Renderer {
    hwnd: HWND,
    factory: ID2D1Factory1,
    style: TextStyle,
    target: Option<Target>,
    scaled: Option<Scaled>,
}

/// 表示倍率に縮小・拡大済みの画像。
/// 大きな写真を毎フレーム高品質補間すると内蔵 GPU では 1 フレーム 25ms ほどかかるので、
/// 倍率が変わったときだけ作り直し、ドラッグ中はこれを等倍で貼るだけにする
struct Scaled {
    source: ID2D1Bitmap,
    scale: f64,
    bmp: ID2D1Bitmap,
}

/// 縮小済みを作る大きさの上限（ピクセル数）。これより大きくなる倍率では毎フレーム補間する
const SCALED_MAX_PIXELS: u64 = 4096 * 4096;

/// GPU へ画像を送るときの 1 回分の大きさ
const UPLOAD_CHUNK_BYTES: usize = 4 * 1024 * 1024;

impl Renderer {
    pub fn new(hwnd: HWND, dpi: u32) -> Result<Renderer> {
        unsafe {
            let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let style = TextStyle::new(dwrite, dpi as f32 / 96.0)?;
            Ok(Renderer { hwnd, factory, style, target: None, scaled: None })
        }
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        if let Ok(s) = unsafe { TextStyle::new(self.style.dwrite.clone(), dpi as f32 / 96.0) } {
            self.style = s;
        }
    }

    fn text_style(&self) -> TextStyle {
        self.style.clone()
    }

    fn target(&mut self) -> Result<&Target> {
        if self.target.is_none() {
            unsafe {
                let (w, h) = client_size(self.hwnd);
                let props = D2D1_RENDER_TARGET_PROPERTIES { dpiX: 96.0, dpiY: 96.0, ..Default::default() };
                let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd: self.hwnd,
                    pixelSize: D2D_SIZE_U { width: w, height: h },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                };
                let rt = self.factory.CreateHwndRenderTarget(&props, &hwnd_props)?;
                let dc: ID2D1DeviceContext = rt.cast()?;
                let text = rt.CreateSolidColorBrush(&TEXT, None)?;
                let panel = rt.CreateSolidColorBrush(&PANEL, None)?;
                let ui = rt.CreateSolidColorBrush(&SLIDER_BG, None)?;
                self.target = Some(Target { rt, dc, text, panel, ui });
            }
        }
        Ok(self.target.as_ref().unwrap())
    }

    /// 描画先を作ってから、GPU に載せられる最大の辺の長さを返す
    pub fn max_bitmap_size(&mut self) -> Option<u32> {
        let t = self.target().ok()?;
        Some(unsafe { t.rt.GetMaximumBitmapSize() })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if let Some(t) = &self.target {
            unsafe {
                let _ = t.rt.Resize(&D2D_SIZE_U { width: w, height: h });
            }
        }
    }

    pub fn upload(&mut self, img: &Image) -> Result<Bitmap> {
        let t = self.target()?;
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        // 一度に全部渡すと、ドライバーが画像と同じ大きさの転送用バッファを抱えたままになる
        // （内蔵 GPU で 2400 万画素 1 枚につき +90MB ほど）。少しずつ送るとそれが小さく済む
        let stride = img.width * 4;
        let bmp = unsafe {
            let bmp = t.rt.CreateBitmap(D2D_SIZE_U { width: img.width, height: img.height }, None, 0, &props)?;
            let rows = (UPLOAD_CHUNK_BYTES / stride as usize).max(1) as u32;
            let mut y = 0;
            while y < img.height {
                let end = (y + rows).min(img.height);
                let rect = D2D_RECT_U { left: 0, top: y, right: img.width, bottom: end };
                let src = img.pixels.as_ptr().add(y as usize * stride as usize);
                bmp.CopyFromMemory(Some(&rect), src as *const _, stride)?;
                y = end;
            }
            bmp
        };
        let geo = Geometry {
            w: img.width,
            h: img.height,
            orig_w: img.orig_w,
            orig_h: img.orig_h,
            orientation: img.orientation,
        };
        Ok(Bitmap { bmp, geo })
    }

    /// 描画する。GPU がリセットされて作り直しが必要になったら false（画像も全部作り直し）
    pub fn draw(&mut self, scene: &Scene) -> bool {
        let text = self.text_style();
        let scaled = self.prepare_scaled(scene);
        let Ok(t) = self.target() else { return true };
        unsafe {
            t.rt.BeginDraw();
            draw_scene(&t.rt, &t.dc, t, &text, scene, scaled.as_ref());
            if let Err(e) = t.rt.EndDraw(None, None) {
                if e.code() == D2DERR_RECREATE_TARGET {
                    self.target = None;
                    self.scaled = None;
                    return false;
                }
            }
        }
        true
    }

    /// 表示中の画像の、今の倍率での縮小・拡大済み画像。等倍のときや大きすぎるときは None
    fn prepare_scaled(&mut self, scene: &Scene) -> Option<ID2D1Bitmap> {
        let (b, view) = scene.image?;
        if b.geo.transform(&view).is_identity_scale() {
            return None;
        }
        if let Some(s) = &self.scaled {
            if s.source == b.bmp && s.scale == view.scale {
                return Some(s.bmp.clone());
            }
        }
        self.scaled = None;
        let t = self.target().ok()?;
        let (dw, dh) = b.geo.display_size();
        let (sw, sh) = ((dw * view.scale).round().max(1.0) as u32, (dh * view.scale).round().max(1.0) as u32);
        unsafe {
            let max = t.rt.GetMaximumBitmapSize();
            if sw > max || sh > max || sw as u64 * sh as u64 > SCALED_MAX_PIXELS {
                return None;
            }
            let size = D2D_SIZE_U { width: sw, height: sh };
            let size_f = D2D_SIZE_F { width: sw as f32, height: sh as f32 };
            let crt = t
                .rt
                .CreateCompatibleRenderTarget(Some(&size_f), Some(&size), None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE)
                .ok()?;
            let cdc: ID2D1DeviceContext = crt.cast().ok()?;
            crt.BeginDraw();
            crt.Clear(Some(&D2D1_COLOR_F::default()));
            let m = b.geo.transform(&View { scale: view.scale, x: 0.0, y: 0.0 });
            cdc.SetTransform(&m.to_d2d());
            cdc.DrawBitmap(&b.bmp, None, 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
            crt.EndDraw(None, None).ok()?;
            let bmp = crt.GetBitmap().ok()?;
            self.scaled = Some(Scaled { source: b.bmp.clone(), scale: view.scale, bmp: bmp.clone() });
            Some(bmp)
        }
    }

    /// 検証用: 画面に出すのと同じ内容を BMP ファイルに書き出す
    /// （リモート接続が切れたデスクトップでは画面キャプチャができないため）
    #[cfg(feature = "snapshot")]
    pub fn snapshot(&mut self, scene: &Scene, path: &std::path::Path) -> Result<()> {
        let (w, h) = client_size(self.hwnd);
        let text = self.text_style();
        let scaled = self.prepare_scaled(scene);
        let t = self.target()?;
        unsafe {
            let size = D2D_SIZE_U { width: w, height: h };
            let crt = t.rt.CreateCompatibleRenderTarget(None, Some(&size), None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE)?;
            let cdc: ID2D1DeviceContext = crt.cast()?;
            crt.BeginDraw();
            draw_scene(&crt, &cdc, t, &text, scene, scaled.as_ref());
            crt.EndDraw(None, None)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let cpu = t.dc.CreateBitmap(size, None, 0, &props)?;
            cpu.CopyFromBitmap(None, &crt.GetBitmap()?, None)?;
            let map = cpu.Map(D2D1_MAP_OPTIONS_READ)?;
            let mut out = Vec::with_capacity(54 + (w * h * 4) as usize);
            out.extend_from_slice(b"BM");
            out.extend_from_slice(&(54 + w * h * 4).to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0]);
            out.extend_from_slice(&54u32.to_le_bytes());
            out.extend_from_slice(&40u32.to_le_bytes());
            out.extend_from_slice(&(w as i32).to_le_bytes());
            out.extend_from_slice(&(-(h as i32)).to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&32u16.to_le_bytes());
            out.extend_from_slice(&[0u8; 24]);
            for y in 0..h as usize {
                let row = std::slice::from_raw_parts(map.bits.add(y * map.pitch as usize), w as usize * 4);
                out.extend_from_slice(row);
            }
            cpu.Unmap()?;
            std::fs::write(path, out).map_err(|_| windows::core::Error::from_thread())?;
        }
        Ok(())
    }
}

#[cfg(feature = "snapshot")]
impl Renderer {
    /// 検証用: 画面と同じ内容をオフスクリーンに n 回描いて、1 回あたりのミリ秒を返す
    pub fn bench(&mut self, scene: &Scene, n: u32) -> Result<f64> {
        let (w, h) = client_size(self.hwnd);
        let text = self.text_style();
        let scaled = self.prepare_scaled(scene);
        let t = self.target()?;
        unsafe {
            let size = D2D_SIZE_U { width: w, height: h };
            let crt = t.rt.CreateCompatibleRenderTarget(None, Some(&size), None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE)?;
            let cdc: ID2D1DeviceContext = crt.cast()?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let cpu = t.dc.CreateBitmap(size, None, 0, &props)?;
            let start = std::time::Instant::now();
            for _ in 0..n {
                crt.BeginDraw();
                draw_scene(&crt, &cdc, t, &text, scene, scaled.as_ref());
                crt.EndDraw(None, None)?;
                // GPU の処理完了まで待つために読み出す
                cpu.CopyFromBitmap(None, &crt.GetBitmap()?, None)?;
                let _ = cpu.Map(D2D1_MAP_OPTIONS_READ)?;
                cpu.Unmap()?;
            }
            Ok(start.elapsed().as_secs_f64() * 1000.0 / n as f64)
        }
    }
}

#[derive(Clone)]
struct TextStyle {
    dwrite: IDWriteFactory,
    /// 画面中央の案内（中央寄せ・折り返しあり）
    format: IDWriteTextFormat,
    font_px: f32,
    /// スライダーのページ番号や設定の値（右寄せ）
    label: IDWriteTextFormat,
    /// 設定の項目名（左寄せ）
    item: IDWriteTextFormat,
    /// 設定の見出し
    title: IDWriteTextFormat,
    /// 歯車などのアイコン
    icon: IDWriteTextFormat,
}

impl TextStyle {
    unsafe fn new(dwrite: IDWriteFactory, k: f32) -> Result<TextStyle> {
        let font_px = FONT_SIZE * k;
        let format = text_format(&dwrite, font_px)?;
        let ui = w!("Yu Gothic UI");
        let label = line_format(&dwrite, ui, slider::LABEL_DIP * k, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let item = line_format(&dwrite, ui, 14.0 * k, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let title = line_format(&dwrite, ui, 16.0 * k, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let icon = line_format(&dwrite, w!("Segoe MDL2 Assets"), 14.0 * k, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_CENTER)?;
        Ok(TextStyle { dwrite, format, font_px, label, item, title, icon })
    }
}

unsafe fn draw_scene(
    rt: &ID2D1RenderTarget,
    dc: &ID2D1DeviceContext,
    t: &Target,
    text: &TextStyle,
    scene: &Scene,
    scaled: Option<&ID2D1Bitmap>,
) {
    rt.SetTransform(&Matrix3x2::identity());
    rt.Clear(Some(&BACKGROUND));

    if let (Some((_, view)), Some(sb)) = (&scene.image, scaled) {
        let size = sb.GetPixelSize();
        let (x, y) = (view.x.round() as f32, view.y.round() as f32);
        let dest = D2D_RECT_F { left: x, top: y, right: x + size.width as f32, bottom: y + size.height as f32 };
        dc.DrawBitmap(sb, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
    } else if let Some((bmp, view)) = &scene.image {
        let m = bmp.geo.transform(view);
        // 等倍でずれもないときは補間しない。それ以外は高品質な補間
        let mode = if m.is_identity_scale() {
            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
        } else {
            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC
        };
        dc.SetTransform(&m.to_d2d());
        dc.DrawBitmap(&bmp.bmp, None, 1.0, mode, None, None);
        dc.SetTransform(&Matrix3x2::identity());
    }

    if let Some(s) = scene.slider {
        draw_slider(rt, t, text, s);
    }

    if let Some(p) = scene.panel {
        draw_panel(rt, t, text, p);
    } else if let Some(s) = scene.text {
        draw_text(rt, t, text, s, scene.area.0, scene.area.1);
    }
}

unsafe fn draw_line(rt: &ID2D1RenderTarget, t: &Target, text: &str, format: &IDWriteTextFormat, rect: &panel::Rect, color: &D2D1_COLOR_F) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let r = D2D_RECT_F { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
    t.ui.SetColor(color);
    rt.DrawText(&wide, format, &r, &t.ui, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
}

unsafe fn fill_pill(rt: &ID2D1RenderTarget, t: &Target, r: &panel::Rect, color: &D2D1_COLOR_F) {
    let radius = (r.bottom - r.top) / 2.0;
    let rect = D2D_RECT_F { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
    t.ui.SetColor(color);
    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: radius, radiusY: radius }, &t.ui);
}

unsafe fn fill_circle(rt: &ID2D1RenderTarget, t: &Target, x: f32, y: f32, r: f32, color: &D2D1_COLOR_F) {
    t.ui.SetColor(color);
    rt.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: r, radiusY: r }, &t.ui);
}

/// 設定パネル。後ろを暗くして、中央に項目を並べる
unsafe fn draw_panel(rt: &ID2D1RenderTarget, t: &Target, style: &TextStyle, p: &PanelScene) {
    let size = rt.GetSize();
    t.ui.SetColor(&DIM);
    rt.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: size.width, bottom: size.height }, &t.ui);

    let l = &p.layout;
    let k = l.scale;
    let pr = D2D_RECT_F { left: l.panel.left, top: l.panel.top, right: l.panel.right, bottom: l.panel.bottom };
    t.ui.SetColor(&PANEL_BG);
    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pr, radiusX: 8.0 * k, radiusY: 8.0 * k }, &t.ui);
    draw_line(rt, t, "設定", &style.title, &l.title, &TEXT_MAIN);
    draw_line(rt, t, GLYPH_CLOSE, &style.icon, &l.close, &TEXT_MAIN);

    for (i, row) in l.rows.iter().enumerate() {
        let a = &row.area;
        if i > 0 {
            t.ui.SetColor(&PANEL_LINE);
            rt.FillRectangle(&D2D_RECT_F { left: row.label.left, top: a.top, right: row.control.right.max(row.value.right), bottom: a.top + k }, &t.ui);
        }
        let on = row.item.enabled(&p.options);
        let fg = if on { TEXT_MAIN } else { TEXT_DIM };
        let accent = if on { SLIDER_FILL } else { TOGGLE_OFF };
        draw_line(rt, t, row.item.label(), &style.item, &row.label, &fg);
        let c = &row.control;
        let cy = (c.top + c.bottom) / 2.0;
        match row.item.kind() {
            Kind::Toggle => {
                let checked = row.item.get(&p.options) >= 0.5;
                fill_pill(rt, t, c, if checked { &accent } else { &TOGGLE_OFF });
                let r = (c.bottom - c.top) / 2.0 - 3.0 * k;
                let x = if checked { c.right - r - 3.0 * k } else { c.left + r + 3.0 * k };
                fill_circle(rt, t, x, cy, r, &SLIDER_THUMB);
            }
            Kind::Slider { .. } => {
                let x = l.slider_x(i, &p.options).unwrap_or(c.left);
                fill_pill(rt, t, c, &SLIDER_TRACK);
                fill_pill(rt, t, &panel::Rect { right: x, ..*c }, &accent);
                fill_circle(rt, t, x, cy, slider::THUMB_DIP * k, if on { &SLIDER_THUMB } else { &TEXT_DIM });
                draw_line(rt, t, &row.item.value_text(&p.options), &style.label, &row.value, &fg);
            }
        }
    }
}

/// 画面下のページスライダー。読んだ分（1 ページ目の端からつまみまで）を色付けする
unsafe fn draw_slider(rt: &ID2D1RenderTarget, t: &Target, style: &TextStyle, s: &SliderScene) {
    let b = &s.bar;
    t.ui.SetColor(&SLIDER_BG);
    rt.FillRectangle(&D2D_RECT_F { left: 0.0, top: b.top, right: b.right, bottom: b.bottom }, &t.ui);

    let cy = ((b.top + b.bottom) / 2.0).round();
    let half = 2.0 * b.scale;
    let track = |left: f32, right: f32| D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F { left, top: cy - half, right, bottom: cy + half },
        radiusX: half,
        radiusY: half,
    };
    t.ui.SetColor(&SLIDER_TRACK);
    rt.FillRoundedRectangle(&track(b.track_left, b.track_right), &t.ui);
    let gear = panel::Rect { left: b.gear_left, top: b.top, right: b.right, bottom: b.bottom };
    draw_line(rt, t, GLYPH_GEAR, &style.icon, &gear, &SLIDER_LABEL);
    if s.count == 0 {
        return;
    }
    let x = b.thumb_x(s.index, s.count);
    let start = b.start_x();
    t.ui.SetColor(&SLIDER_FILL);
    rt.FillRoundedRectangle(&track(x.min(start), x.max(start)), &t.ui);
    fill_circle(rt, t, x, cy, slider::THUMB_DIP * b.scale, &SLIDER_THUMB);

    let rect = panel::Rect { left: b.label_left, top: b.top, right: b.label_right, bottom: b.bottom };
    draw_line(rt, t, &format!("{} / {}", s.index + 1, s.count), &style.label, &rect, &SLIDER_LABEL);
}

/// 画面中央に、半透明の背景つきで文字を出す
unsafe fn draw_text(rt: &ID2D1RenderTarget, t: &Target, style: &TextStyle, text: &str, w: f32, h: f32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let max_w = (w * 0.9).max(100.0);
    let Ok(layout) = style.dwrite.CreateTextLayout(&wide, &style.format, max_w, h.max(1.0)) else { return };
    let mut m = DWRITE_TEXT_METRICS::default();
    if layout.GetMetrics(&mut m).is_err() {
        return;
    }
    let origin = Vector2 { X: (w - max_w) / 2.0, Y: ((h - m.height) / 2.0).round() };
    let pad = style.font_px * 0.8;
    let rect = D2D_RECT_F {
        left: origin.X + m.left - pad,
        top: origin.Y + m.top - pad * 0.6,
        right: origin.X + m.left + m.width + pad,
        bottom: origin.Y + m.top + m.height + pad * 0.6,
    };
    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: pad * 0.5, radiusY: pad * 0.5 }, &t.panel);
    rt.DrawTextLayout(origin, &layout, &t.text, D2D1_DRAW_TEXT_OPTIONS_NONE);
}

/// 1 行の文字。上下中央・折り返さない
unsafe fn line_format(
    dwrite: &IDWriteFactory,
    family: windows::core::PCWSTR,
    px: f32,
    weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT,
) -> Result<IDWriteTextFormat> {
    let f = dwrite.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, px, w!("ja-jp"))?;
    f.SetTextAlignment(align)?;
    f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
    f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
    Ok(f)
}

unsafe fn text_format(dwrite: &IDWriteFactory, px: f32) -> Result<IDWriteTextFormat> {
    let f = dwrite.CreateTextFormat(
        w!("Yu Gothic UI"),
        None,
        DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        px,
        w!("ja-jp"),
    )?;
    f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
    Ok(f)
}

pub fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 元画像 4x2 の四隅が、EXIF の向きごとに表示上のどこへ行くか
    fn corners(o: u8) -> Vec<(f64, f64)> {
        let b = Geometry {
            w: 4,
            h: 2,
            orig_w: 4,
            orig_h: 2,
            orientation: o,
        };
        let m = b.transform(&View::default()).0;
        let r = [(0.0, 0.0), (4.0, 0.0), (0.0, 2.0)]
            .iter()
            .map(|&(x, y)| (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5]))
            .collect();
        r
    }

    #[test]
    fn exif_orientation() {
        // 左上, 右上, 左下 の行き先
        assert_eq!(corners(1), [(0.0, 0.0), (4.0, 0.0), (0.0, 2.0)]);
        assert_eq!(corners(2), [(4.0, 0.0), (0.0, 0.0), (4.0, 2.0)]);
        assert_eq!(corners(3), [(4.0, 2.0), (0.0, 2.0), (4.0, 0.0)]);
        assert_eq!(corners(4), [(0.0, 2.0), (4.0, 2.0), (0.0, 0.0)]);
        // 6: 時計回りに 90 度回すと正しい向き。表示サイズは 2x4
        assert_eq!(corners(6), [(2.0, 0.0), (2.0, 4.0), (0.0, 0.0)]);
        // 8: 反時計回りに 90 度
        assert_eq!(corners(8), [(0.0, 4.0), (0.0, 0.0), (2.0, 4.0)]);
        // 5: 転置, 7: 反転置
        assert_eq!(corners(5), [(0.0, 0.0), (0.0, 4.0), (2.0, 0.0)]);
        assert_eq!(corners(7), [(2.0, 4.0), (2.0, 0.0), (0.0, 4.0)]);
    }

    #[test]
    fn downscaled_bitmap_maps_back_to_original_size() {
        let b = Geometry {
            w: 100,
            h: 50,
            orig_w: 400,
            orig_h: 200,
            orientation: 1,
        };
        let m = b.transform(&View { scale: 0.5, x: 10.0, y: 20.0 });
        assert_eq!(m.0, [2.0, 0.0, 0.0, 2.0, 10.0, 20.0]);
    }
}
