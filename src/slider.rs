//! 画面下のページスライダーの位置計算。
//! 既定は NeeView と同じく本を開く方向（右開き）で、1 枚目が右端、最後が左端。設定で左から右にもできる。
//! 右端には設定パネルを開く歯車を置く。

/// バーの高さ（DIP）。NeeView の既定と同じ
const HEIGHT_DIP: f32 = 25.0;
/// ページ番号の文字の大きさ（DIP）
pub const LABEL_DIP: f32 = 13.0;
/// つまみの半径（DIP）
pub const THUMB_DIP: f32 = 6.0;
const PAD_DIP: f32 = 12.0;

/// 座標はクライアント領域の物理ピクセル
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub top: f32,
    pub bottom: f32,
    pub right: f32,
    /// つまみが動く範囲
    pub track_left: f32,
    pub track_right: f32,
    /// ページ番号を書く範囲
    pub label_left: f32,
    pub label_right: f32,
    /// 歯車（設定パネルを開く）の左端。右端はバーの右端
    pub gear_left: f32,
    /// 左から右へ進むか
    pub ltr: bool,
    /// DPI の倍率（96dpi で 1.0）
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Track,
    Gear,
}

pub fn height(scale: f32) -> f32 {
    (HEIGHT_DIP * scale).round()
}

/// ウィンドウ下端に置くバー。右から「歯車」「n / count」の幅を取り、残りをつまみの範囲にする
pub fn layout(w: f32, h: f32, scale: f32, count: usize, ltr: bool) -> Bar {
    let pad = PAD_DIP * scale;
    let height = height(scale);
    let gear_left = w - height * 1.4;
    // 「count / count」が入る幅。数字は 1 文字 0.6em 程度
    let chars = count.max(1).to_string().len() * 2 + 3;
    let label_w = chars as f32 * LABEL_DIP * 0.6 * scale;
    let label_right = gear_left - pad * 0.5;
    let label_left = label_right - label_w;
    Bar {
        top: h - height,
        bottom: h,
        right: w,
        track_left: pad,
        track_right: (label_left - pad).max(pad + 1.0),
        label_left,
        label_right,
        gear_left,
        ltr,
        scale,
    }
}

impl Bar {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if y < self.top || y >= self.bottom || x < 0.0 || x >= self.right {
            None
        } else if x >= self.gear_left {
            Some(Hit::Gear)
        } else {
            Some(Hit::Track)
        }
    }

    /// index ページ目のつまみの x 座標
    pub fn thumb_x(&self, index: usize, count: usize) -> f32 {
        let p = if count <= 1 { 0.0 } else { index as f32 / (count - 1) as f32 };
        let r = if self.ltr { p } else { 1.0 - p };
        self.track_left + r * (self.track_right - self.track_left)
    }

    /// 1 ページ目のつまみがある側の端の x 座標（読んだ分の色付けはここから）
    pub fn start_x(&self) -> f32 {
        if self.ltr {
            self.track_left
        } else {
            self.track_right
        }
    }

    /// x の位置にあたるページ
    pub fn index_at(&self, x: f32, count: usize) -> usize {
        if count <= 1 {
            return 0;
        }
        let r = ((x - self.track_left) / (self.track_right - self.track_left)).clamp(0.0, 1.0);
        let p = if self.ltr { r } else { 1.0 - r };
        (p * (count - 1) as f32).round() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ltr: bool) -> Bar {
        layout(1000.0, 800.0, 1.0, 100, ltr)
    }

    #[test]
    fn sits_at_bottom() {
        let b = bar(false);
        assert_eq!((b.top, b.bottom), (775.0, 800.0));
        assert_eq!(b.hit(500.0, 790.0), Some(Hit::Track));
        assert_eq!(b.hit(500.0, 770.0), None);
        assert_eq!(b.hit(995.0, 790.0), Some(Hit::Gear));
        assert_eq!(layout(1000.0, 800.0, 1.5, 100, false).top, 800.0 - 38.0);
    }

    #[test]
    fn right_to_left_by_default() {
        let b = bar(false);
        assert_eq!(b.thumb_x(0, 100), b.track_right);
        assert_eq!(b.thumb_x(99, 100), b.track_left);
        assert_eq!(b.start_x(), b.track_right);
        // 100 枚中 50 枚目はほぼ真ん中
        let mid = (b.track_left + b.track_right) / 2.0;
        assert!((b.thumb_x(49, 100) - mid).abs() < (b.track_right - b.track_left) / 99.0);
    }

    #[test]
    fn left_to_right_option() {
        let b = bar(true);
        assert_eq!(b.thumb_x(0, 100), b.track_left);
        assert_eq!(b.thumb_x(99, 100), b.track_right);
        assert_eq!(b.start_x(), b.track_left);
        assert_eq!(b.index_at(b.track_left - 50.0, 100), 0);
    }

    #[test]
    fn click_position_maps_back_to_page() {
        for ltr in [false, true] {
            let b = bar(ltr);
            for i in [0, 1, 49, 50, 98, 99] {
                assert_eq!(b.index_at(b.thumb_x(i, 100), 100), i);
            }
        }
        // トラックの外側は端のページ
        let b = bar(false);
        assert_eq!(b.index_at(-50.0, 100), 99);
        assert_eq!(b.index_at(b.right, 100), 0);
    }

    #[test]
    fn single_page() {
        let b = layout(1000.0, 800.0, 1.0, 1, false);
        assert_eq!(b.thumb_x(0, 1), b.track_right);
        assert_eq!(b.index_at(10.0, 1), 0);
    }

    #[test]
    fn label_and_gear_leave_room_for_track() {
        let b = layout(1000.0, 800.0, 1.0, 12345, false);
        assert!(b.track_right < b.label_left);
        assert!(b.label_right < b.gear_left);
        assert!(b.gear_left < 1000.0);
    }
}
