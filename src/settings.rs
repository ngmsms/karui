//! %LOCALAPPDATA%\karui\settings.txt に「key=value」で保存する設定

use std::path::PathBuf;

use crate::view::Mode;

pub struct Settings {
    pub mode: Mode,
    /// 通常時（最大化していないとき）のウィンドウ位置。left, top, right, bottom
    pub rect: Option<[i32; 4]>,
    pub maximized: bool,
    pub options: Options,
}

/// 設定パネルで変えられるもの
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// はみ出した画像をホイールでスクロールする（オフならホイールは常にページ送り）
    pub wheel_scroll: bool,
    /// ホイール 1 ノッチでスクロールする量。画像を置く範囲の高さ（横だけはみ出すときは幅）に対する %
    pub scroll_percent: u32,
    /// スクロールにかける時間（ミリ秒）。0 ならすぐ動く
    pub scroll_ms: u32,
    /// 次の画像を読み込んでいる間のホイールを無視する
    pub wait_loading: bool,
    /// ページスライダーを左から右へ進める（オフなら NeeView の既定どおり右開き）
    pub slider_ltr: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { wheel_scroll: true, scroll_percent: 40, scroll_ms: 200, wait_loading: true, slider_ltr: false }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings { mode: Mode::FitWindow, rect: None, maximized: false, options: Options::default() }
    }
}

fn path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("karui").join("settings.txt"))
}

pub fn load() -> Settings {
    path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse(&t)).unwrap_or_default()
}

fn parse(text: &str) -> Settings {
    let mut s = Settings::default();
    let o = &mut s.options;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        let flag = value == "1";
        let num = value.parse::<u32>().ok();
        match key.trim() {
            "mode" => s.mode = Mode::from_key(value).unwrap_or(s.mode),
            "window" => {
                let v: Vec<i32> = value.split(',').filter_map(|n| n.trim().parse().ok()).collect();
                if let [l, t, r, b] = v[..] {
                    if r > l && b > t {
                        s.rect = Some([l, t, r, b]);
                    }
                }
            }
            "maximized" => s.maximized = flag,
            "wheel_scroll" => o.wheel_scroll = flag,
            "scroll_percent" => o.scroll_percent = num.unwrap_or(o.scroll_percent).clamp(5, 100),
            "scroll_ms" => o.scroll_ms = num.unwrap_or(o.scroll_ms).min(1000),
            "wait_loading" => o.wait_loading = flag,
            "slider_ltr" => o.slider_ltr = flag,
            _ => {}
        }
    }
    s
}

impl Settings {
    fn to_text(&self) -> String {
        let o = &self.options;
        let mut text = format!(
            "mode={}\nmaximized={}\nwheel_scroll={}\nscroll_percent={}\nscroll_ms={}\nwait_loading={}\nslider_ltr={}\n",
            self.mode.key(),
            self.maximized as u8,
            o.wheel_scroll as u8,
            o.scroll_percent,
            o.scroll_ms,
            o.wait_loading as u8,
            o.slider_ltr as u8,
        );
        if let Some([l, t, r, b]) = self.rect {
            text += &format!("window={l},{t},{r},{b}\n");
        }
        text
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, self.to_text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            mode: Mode::FitWidth,
            rect: Some([1, 2, 300, 400]),
            maximized: true,
            options: Options { wheel_scroll: false, scroll_percent: 65, scroll_ms: 0, wait_loading: false, slider_ltr: true },
        };
        let r = parse(&s.to_text());
        assert_eq!((r.mode, r.rect, r.maximized, r.options), (s.mode, s.rect, s.maximized, s.options));
    }

    #[test]
    fn old_file_gets_defaults() {
        // 設定パネルを入れる前の形式
        let r = parse("mode=height\nmaximized=0\nwindow=64,71,1344,931\n");
        assert_eq!(r.mode, Mode::FitHeight);
        assert_eq!(r.options, Options::default());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let r = parse("scroll_percent=0\nscroll_ms=99999\n");
        assert_eq!((r.options.scroll_percent, r.options.scroll_ms), (5, 1000));
    }
}
