//! 表示モードと、画像の拡大率・位置の計算。座標はすべてクライアント領域の物理ピクセル。

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Original,
    FitWindow,
    FitHeight,
    FitWidth,
}

// NeeView の「表示サイズを切り替える」と同じ並び（使わない「いっぱいに広げる」「面積を合わせる」は除いた）
const CYCLE: [Mode; 4] = [Mode::Original, Mode::FitWindow, Mode::FitHeight, Mode::FitWidth];

impl Mode {
    pub fn next(self) -> Mode {
        CYCLE[(self.pos() + 1) % CYCLE.len()]
    }

    pub fn prev(self) -> Mode {
        CYCLE[(self.pos() + CYCLE.len() - 1) % CYCLE.len()]
    }

    fn pos(self) -> usize {
        CYCLE.iter().position(|&m| m == self).unwrap()
    }

    /// 切り替え時に出す表示名（NeeView の日本語表記に合わせている）
    pub fn label(self) -> &'static str {
        match self {
            Mode::Original => "オリジナル サイズ",
            Mode::FitWindow => "ウィンドウに合わせる",
            Mode::FitHeight => "高さをウィンドウに合わせる",
            Mode::FitWidth => "幅をウィンドウに合わせる",
        }
    }

    /// 設定ファイルに書く名前
    pub fn key(self) -> &'static str {
        match self {
            Mode::Original => "original",
            Mode::FitWindow => "window",
            Mode::FitHeight => "height",
            Mode::FitWidth => "width",
        }
    }

    pub fn from_key(s: &str) -> Option<Mode> {
        CYCLE.into_iter().find(|m| m.key() == s)
    }
}

pub type Size = (f64, f64);

/// 画像をどの倍率でどこに置くか。x, y は画像左上の位置
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: f64,
    pub x: f64,
    pub y: f64,
}

impl Default for View {
    fn default() -> Self {
        View { scale: 1.0, x: 0.0, y: 0.0 }
    }
}

impl View {
    /// ページを開いたときの配置。forward は次のページへ進んできたかどうか。
    /// 右開き（NeeView の既定）なので、ウィンドウからはみ出す場合は
    /// 進んだときは右上、戻ったときは左下から見せる。収まる向きは中央に置く。
    pub fn open(mode: Mode, img: Size, win: Size, forward: bool) -> View {
        let scale = fit_scale(mode, img, win);
        let (w, h) = (img.0 * scale, img.1 * scale);
        let mut v = View {
            scale,
            x: if forward { win.0 - w } else { 0.0 },
            y: if forward { 0.0 } else { win.1 - h },
        };
        v.clamp(img, win);
        v
    }

    /// ドラッグでずらす。はみ出していない向きには動かない
    pub fn pan(&mut self, dx: f64, dy: f64, img: Size, win: Size) {
        self.x += dx;
        self.y += dy;
        self.clamp(img, win);
    }

    /// ウィンドウサイズが変わったとき。画面中央に見えていた箇所をなるべく保つ
    pub fn resized(&self, mode: Mode, img: Size, old: Size, new: Size) -> View {
        let (w, h) = (img.0 * self.scale, img.1 * self.scale);
        let fx = if w > 0.0 { (old.0 / 2.0 - self.x) / w } else { 0.5 };
        let fy = if h > 0.0 { (old.1 / 2.0 - self.y) / h } else { 0.5 };
        let scale = fit_scale(mode, img, new);
        let mut v = View {
            scale,
            x: new.0 / 2.0 - fx * img.0 * scale,
            y: new.1 / 2.0 - fy * img.1 * scale,
        };
        v.clamp(img, new);
        v
    }

    /// ホイール 1 ノッチ分のスクロール先。進めなければ None（ページを移る）。
    /// NeeView の N 字スクロールと同じ順にたどる: 縦に下端まで → 読む方向（右開きなので左）へ
    /// 1 画面ずらして上端へ → … → 左下で終わり。戻るときはその逆。
    /// step は 1 ノッチの量（横, 縦）。横だけはみ出すときは横に進む。残りが margin 未満なら端に着いたとみなす
    pub fn scroll_step(&self, img: Size, win: Size, forward: bool, step: (f64, f64), margin: f64) -> Option<View> {
        let (w, h) = (img.0 * self.scale, img.1 * self.scale);
        let (min_x, min_y) = (win.0 - w, win.1 - h);
        let (over_x, over_y) = (w > win.0, h > win.1);
        let mut v = *self;
        if forward {
            if over_y && self.y - min_y > margin {
                v.y = (self.y - step.1).max(min_y);
            } else if over_x && -self.x > margin {
                if over_y {
                    v.x = (self.x + win.0).min(0.0);
                    v.y = 0.0;
                } else {
                    v.x = (self.x + step.0).min(0.0);
                }
            } else {
                return None;
            }
        } else if over_y && -self.y > margin {
            v.y = (self.y + step.1).min(0.0);
        } else if over_x && self.x - min_x > margin {
            if over_y {
                v.x = (self.x - win.0).max(min_x);
                v.y = min_y;
            } else {
                v.x = (self.x - step.0).max(min_x);
            }
        } else {
            return None;
        }
        Some(v)
    }

    fn clamp(&mut self, img: Size, win: Size) {
        let (w, h) = (img.0 * self.scale, img.1 * self.scale);
        self.x = if w <= win.0 { (win.0 - w) / 2.0 } else { self.x.clamp(win.0 - w, 0.0) };
        self.y = if h <= win.1 { (win.1 - h) / 2.0 } else { self.y.clamp(win.1 - h, 0.0) };
    }
}

/// NeeView の既定どおり、小さい画像は拡大し、大きい画像は縮小する
pub fn fit_scale(mode: Mode, img: Size, win: Size) -> f64 {
    if img.0 <= 0.0 || img.1 <= 0.0 || win.0 <= 0.0 || win.1 <= 0.0 {
        return 1.0;
    }
    match mode {
        Mode::Original => 1.0,
        Mode::FitWindow => (win.0 / img.0).min(win.1 / img.1),
        Mode::FitHeight => win.1 / img.1,
        Mode::FitWidth => win.0 / img.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: Size = (1000.0, 800.0);

    #[test]
    fn cycle_order_matches_neeview() {
        assert_eq!(Mode::Original.next(), Mode::FitWindow);
        assert_eq!(Mode::FitWindow.next(), Mode::FitHeight);
        assert_eq!(Mode::FitHeight.next(), Mode::FitWidth);
        assert_eq!(Mode::FitWidth.next(), Mode::Original);
        assert_eq!(Mode::Original.prev(), Mode::FitWidth);
    }

    #[test]
    fn mode_key_roundtrip() {
        for m in CYCLE {
            assert_eq!(Mode::from_key(m.key()), Some(m));
        }
        assert_eq!(Mode::from_key("nope"), None);
    }

    #[test]
    fn fit_scales() {
        let img = (2000.0, 1000.0);
        assert_eq!(fit_scale(Mode::Original, img, WIN), 1.0);
        assert_eq!(fit_scale(Mode::FitWindow, img, WIN), 0.5);
        assert_eq!(fit_scale(Mode::FitHeight, img, WIN), 0.8);
        assert_eq!(fit_scale(Mode::FitWidth, img, WIN), 0.5);
        // 小さい画像は拡大される
        assert_eq!(fit_scale(Mode::FitWindow, (100.0, 100.0), WIN), 8.0);
    }

    #[test]
    fn small_image_is_centered_and_locked() {
        let img = (200.0, 100.0);
        let mut v = View::open(Mode::Original, img, WIN, true);
        assert_eq!((v.x, v.y), (400.0, 350.0));
        v.pan(50.0, -30.0, img, WIN);
        assert_eq!((v.x, v.y), (400.0, 350.0));
    }

    #[test]
    fn large_image_opens_top_right_forward_bottom_left_backward() {
        let img = (3000.0, 2000.0);
        let f = View::open(Mode::Original, img, WIN, true);
        assert_eq!((f.x, f.y), (1000.0 - 3000.0, 0.0));
        let b = View::open(Mode::Original, img, WIN, false);
        assert_eq!((b.x, b.y), (0.0, 800.0 - 2000.0));
    }

    #[test]
    fn pan_stops_at_edges() {
        let img = (3000.0, 2000.0);
        let mut v = View::open(Mode::Original, img, WIN, true);
        v.pan(-10_000.0, 10_000.0, img, WIN);
        assert_eq!((v.x, v.y), (-2000.0, 0.0));
        v.pan(10_000.0, -10_000.0, img, WIN);
        assert_eq!((v.x, v.y), (0.0, -1200.0));
    }

    #[test]
    fn fit_width_scrolls_only_vertically() {
        let img = (1000.0, 4000.0);
        let mut v = View::open(Mode::FitWidth, img, WIN, true);
        assert_eq!((v.scale, v.x, v.y), (1.0, 0.0, 0.0));
        v.pan(300.0, -500.0, img, WIN);
        assert_eq!((v.x, v.y), (0.0, -500.0));
    }

    /// forward 向きに、進めなくなるまでスクロールした位置の列
    fn scroll_all(mut v: View, img: Size, forward: bool) -> Vec<(f64, f64)> {
        let mut out = vec![];
        while let Some(n) = v.scroll_step(img, WIN, forward, (300.0, 300.0), 10.0) {
            v = n;
            out.push((v.x, v.y));
            assert!(out.len() < 100);
        }
        out
    }

    #[test]
    fn wheel_scrolls_tall_image_down_then_stops() {
        let img = (1000.0, 1800.0);
        let v = View::open(Mode::Original, img, WIN, true);
        // 縦だけはみ出す。最後は端でぴったり止まる
        assert_eq!(scroll_all(v, img, true), [(0.0, -300.0), (0.0, -600.0), (0.0, -900.0), (0.0, -1000.0)]);
        let end = View { y: -1000.0, ..v };
        assert_eq!(end.scroll_step(img, WIN, true, (300.0, 300.0), 10.0), None);
        // 戻る向き
        assert_eq!(scroll_all(end, img, false).last(), Some(&(0.0, 0.0)));
    }

    #[test]
    fn wheel_scrolls_wide_image_right_to_left() {
        let img = (1600.0, 800.0);
        let v = View::open(Mode::Original, img, WIN, true);
        assert_eq!((v.x, v.y), (-600.0, 0.0));
        assert_eq!(scroll_all(v, img, true), [(-300.0, 0.0), (0.0, 0.0)]);
        let back = View::open(Mode::Original, img, WIN, false);
        assert_eq!(scroll_all(back, img, false), [(-300.0, 0.0), (-600.0, 0.0)]);
    }

    #[test]
    fn wheel_n_scroll_covers_both_directions() {
        let img = (1500.0, 1400.0);
        let v = View::open(Mode::Original, img, WIN, true);
        let path = scroll_all(v, img, true);
        // 右の列を下へ → 左へ 1 画面ずらして上端 → 下へ
        assert_eq!(path, [(-500.0, -300.0), (-500.0, -600.0), (0.0, 0.0), (0.0, -300.0), (0.0, -600.0)]);
        // 戻る向きは、前のページを開いたときの位置（左下）から逆にたどって右上で終わる
        let back = View::open(Mode::Original, img, WIN, false);
        assert_eq!(scroll_all(back, img, false).last(), Some(&(-500.0, 0.0)));
    }

    #[test]
    fn wheel_ignores_tiny_remainder_and_fitting_images() {
        let img = (1000.0, 805.0);
        let v = View::open(Mode::Original, img, WIN, true);
        assert_eq!(v.scroll_step(img, WIN, true, (300.0, 300.0), 10.0), None);
        let fit = View::open(Mode::FitWindow, (3000.0, 2000.0), WIN, true);
        assert_eq!(fit.scroll_step((3000.0, 2000.0), WIN, true, (300.0, 300.0), 10.0), None);
    }

    #[test]
    fn resize_keeps_center_point() {
        let img = (4000.0, 4000.0);
        let mut v = View::open(Mode::Original, img, WIN, true);
        v.pan(1500.0, -1600.0, img, WIN);
        // 画面中央に見えている画像上の点
        let before = (WIN.0 / 2.0 - v.x, WIN.1 / 2.0 - v.y);
        let new = (1200.0, 900.0);
        let r = v.resized(Mode::Original, img, WIN, new);
        assert_eq!((new.0 / 2.0 - r.x, new.1 / 2.0 - r.y), before);
    }
}
