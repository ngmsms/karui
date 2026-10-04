//! 設定パネル。画面中央に重ねて出す。項目の定義と、位置計算・クリック判定。
//! 開いているときだけ描くので、起動や普段の表示の速さには関係しない。

use crate::settings::Options;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    WheelScroll,
    ScrollAmount,
    ScrollTime,
    WaitLoading,
    SliderLtr,
}

pub const ITEMS: [Item; 5] = [Item::WheelScroll, Item::ScrollAmount, Item::ScrollTime, Item::WaitLoading, Item::SliderLtr];

pub enum Kind {
    Toggle,
    /// 値の範囲と刻み
    Slider { min: f64, max: f64, step: f64 },
}

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::WheelScroll => "はみ出した画像をホイールでスクロール",
            Item::ScrollAmount => "1 ノッチのスクロール量",
            Item::ScrollTime => "スクロールにかける時間",
            Item::WaitLoading => "読み込み中はホイールを無視",
            Item::SliderLtr => "ページスライダーを左から右へ",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Item::ScrollAmount => Kind::Slider { min: 10.0, max: 100.0, step: 5.0 },
            Item::ScrollTime => Kind::Slider { min: 0.0, max: 500.0, step: 50.0 },
            _ => Kind::Toggle,
        }
    }

    /// トグルはオンなら 1.0
    pub fn get(self, o: &Options) -> f64 {
        match self {
            Item::WheelScroll => o.wheel_scroll as u8 as f64,
            Item::ScrollAmount => o.scroll_percent as f64,
            Item::ScrollTime => o.scroll_ms as f64,
            Item::WaitLoading => o.wait_loading as u8 as f64,
            Item::SliderLtr => o.slider_ltr as u8 as f64,
        }
    }

    pub fn set(self, o: &mut Options, v: f64) {
        match self {
            Item::WheelScroll => o.wheel_scroll = v >= 0.5,
            Item::ScrollAmount => o.scroll_percent = v.round() as u32,
            Item::ScrollTime => o.scroll_ms = v.round() as u32,
            Item::WaitLoading => o.wait_loading = v >= 0.5,
            Item::SliderLtr => o.slider_ltr = v >= 0.5,
        }
    }

    /// スライダーの右に出す値
    pub fn value_text(self, o: &Options) -> String {
        match self {
            Item::ScrollAmount => format!("画面の {}%", o.scroll_percent),
            Item::ScrollTime if o.scroll_ms == 0 => "なし".to_string(),
            Item::ScrollTime => format!("{:.2} 秒", o.scroll_ms as f64 / 1000.0),
            _ => String::new(),
        }
    }

    /// スクロールに関する項目は、ホイールでスクロールしないときは使わない
    pub fn enabled(self, o: &Options) -> bool {
        match self {
            Item::ScrollAmount | Item::ScrollTime => o.wheel_scroll,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

pub struct Row {
    pub item: Item,
    pub label: Rect,
    /// トグルのつまみの溝、またはスライダーの溝
    pub control: Rect,
    pub value: Rect,
    /// クリックを受ける範囲（行全体）
    pub area: Rect,
}

pub struct Layout {
    pub panel: Rect,
    pub title: Rect,
    pub close: Rect,
    pub rows: Vec<Row>,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Outside,
    Close,
    Row(usize),
    /// パネルの中の、何もないところ
    Inside,
}

const WIDTH_DIP: f32 = 500.0;
const PAD_DIP: f32 = 22.0;
const TITLE_DIP: f32 = 52.0;
const ROW_DIP: f32 = 46.0;
const VALUE_DIP: f32 = 86.0;
const TOGGLE_W_DIP: f32 = 40.0;
const TOGGLE_H_DIP: f32 = 20.0;
const SLIDER_W_DIP: f32 = 150.0;

/// ウィンドウ（w × h）の中央に置く
pub fn layout(w: f32, h: f32, scale: f32) -> Layout {
    let k = scale;
    let pad = PAD_DIP * k;
    let width = (WIDTH_DIP * k).min(w - 16.0 * k).max(200.0 * k);
    let height = TITLE_DIP * k + ROW_DIP * k * ITEMS.len() as f32 + pad * 0.6;
    let left = ((w - width) / 2.0).round();
    let top = ((h - height) / 2.0).round().max(0.0);
    let panel = Rect { left, top, right: left + width, bottom: top + height };
    let title = Rect { left: left + pad, top, right: panel.right - pad, bottom: top + TITLE_DIP * k };
    let close = Rect { left: panel.right - TITLE_DIP * k, top, right: panel.right, bottom: top + TITLE_DIP * k };

    let rows = ITEMS
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let top = title.bottom + ROW_DIP * k * i as f32;
            let bottom = top + ROW_DIP * k;
            let cy = (top + bottom) / 2.0;
            let right = panel.right - pad;
            let (control, value) = match item.kind() {
                Kind::Toggle => {
                    let (tw, th) = (TOGGLE_W_DIP * k, TOGGLE_H_DIP * k);
                    (Rect { left: right - tw, top: cy - th / 2.0, right, bottom: cy + th / 2.0 }, Rect::default())
                }
                Kind::Slider { .. } => {
                    let value = Rect { left: right - VALUE_DIP * k, top, right, bottom };
                    let sl = value.left - SLIDER_W_DIP * k;
                    (Rect { left: sl, top: cy - 2.0 * k, right: value.left - 12.0 * k, bottom: cy + 2.0 * k }, value)
                }
            };
            Row {
                item,
                label: Rect { left: left + pad, top, right: control.left - 12.0 * k, bottom },
                control,
                value,
                area: Rect { left, top, right: panel.right, bottom },
            }
        })
        .collect();
    Layout { panel, title, close, rows, scale }
}

impl Layout {
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if !self.panel.contains(x, y) {
            Hit::Outside
        } else if self.close.contains(x, y) {
            Hit::Close
        } else if let Some(i) = self.rows.iter().position(|r| r.area.contains(x, y)) {
            Hit::Row(i)
        } else {
            Hit::Inside
        }
    }

    /// スライダーの行で x の位置にあたる値（刻みに丸める）
    pub fn slider_value(&self, row: usize, x: f32) -> Option<f64> {
        let r = self.rows.get(row)?;
        let Kind::Slider { min, max, step } = r.item.kind() else { return None };
        let t = ((x - r.control.left) / (r.control.right - r.control.left)).clamp(0.0, 1.0) as f64;
        Some((((min + t * (max - min)) / step).round() * step).clamp(min, max))
    }

    /// スライダーのつまみの x 座標
    pub fn slider_x(&self, row: usize, o: &Options) -> Option<f32> {
        let r = self.rows.get(row)?;
        let Kind::Slider { min, max, .. } = r.item.kind() else { return None };
        let t = ((r.item.get(o) - min) / (max - min)).clamp(0.0, 1.0) as f32;
        Some(r.control.left + t * (r.control.right - r.control.left))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_and_rows_inside_panel() {
        let l = layout(1200.0, 800.0, 1.0);
        assert_eq!(l.panel.left, 350.0);
        assert_eq!((l.panel.left + l.panel.right) / 2.0, 600.0);
        for r in &l.rows {
            assert!(r.area.top >= l.panel.top && r.area.bottom <= l.panel.bottom);
            assert!(r.label.right < r.control.left);
        }
    }

    #[test]
    fn hit_testing() {
        let l = layout(1200.0, 800.0, 1.0);
        assert_eq!(l.hit(10.0, 10.0), Hit::Outside);
        assert_eq!(l.hit(l.close.left + 5.0, l.close.top + 5.0), Hit::Close);
        let r = &l.rows[2];
        assert_eq!(l.hit(r.control.left + 1.0, (r.area.top + r.area.bottom) / 2.0), Hit::Row(2));
    }

    #[test]
    fn slider_value_snaps_and_roundtrips() {
        let l = layout(1200.0, 800.0, 1.0);
        let i = ITEMS.iter().position(|&x| x == Item::ScrollAmount).unwrap();
        let c = l.rows[i].control;
        assert_eq!(l.slider_value(i, c.left - 100.0), Some(10.0));
        assert_eq!(l.slider_value(i, c.right + 100.0), Some(100.0));
        let mut o = Options::default();
        for v in [10.0, 40.0, 55.0, 100.0] {
            Item::ScrollAmount.set(&mut o, v);
            let x = l.slider_x(i, &o).unwrap();
            assert_eq!(l.slider_value(i, x), Some(v));
        }
        // トグルの行はスライダーではない
        assert_eq!(l.slider_value(0, c.left), None);
    }

    #[test]
    fn items_read_and_write_options() {
        let mut o = Options::default();
        for item in ITEMS {
            let v = item.get(&o);
            item.set(&mut o, v);
        }
        assert_eq!(o, Options::default());
        Item::SliderLtr.set(&mut o, 1.0);
        assert!(o.slider_ltr);
        assert_eq!(Item::ScrollTime.value_text(&Options { scroll_ms: 0, ..o }), "なし");
        assert_eq!(Item::ScrollAmount.value_text(&o), "画面の 40%");
    }

    #[test]
    fn narrow_window() {
        let l = layout(300.0, 400.0, 1.0);
        assert!(l.panel.left >= 0.0 && l.panel.right <= 300.0);
    }
}
