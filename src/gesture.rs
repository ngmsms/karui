//! 右ボタンを押しながら動かすマウスジェスチャー。
//! NeeView と同じく、一定の距離（既定 30）動くごとに上下左右のどれかを読み取り、つなげて判定する。

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn arrow(self) -> char {
        match self {
            Dir::Up => '↑',
            Dir::Down => '↓',
            Dir::Left => '←',
            Dir::Right => '→',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    ToggleFullscreen,
    Reload,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Command::ToggleFullscreen => "全画面表示",
            Command::Reload => "再読み込み",
        }
    }
}

/// 割り当て。↑ は NeeView の既定と同じ
pub fn command(dirs: &[Dir]) -> Option<Command> {
    match dirs {
        [Dir::Up] => Some(Command::ToggleFullscreen),
        [Dir::Up, Dir::Down] => Some(Command::Reload),
        _ => None,
    }
}

pub struct Tracker {
    anchor: (f32, f32),
    threshold: f32,
    pub dirs: Vec<Dir>,
}

impl Tracker {
    pub fn new(p: (f32, f32), threshold: f32) -> Tracker {
        Tracker { anchor: p, threshold, dirs: Vec::new() }
    }

    /// マウスが p に動いた。読み取った向きが増えたら true
    pub fn moved(&mut self, p: (f32, f32)) -> bool {
        let (dx, dy) = (p.0 - self.anchor.0, p.1 - self.anchor.1);
        if dx.abs() < self.threshold && dy.abs() < self.threshold {
            return false;
        }
        // 斜めのときは大きく動いた向き
        let d = if dx.abs() >= dy.abs() {
            if dx > 0.0 {
                Dir::Right
            } else {
                Dir::Left
            }
        } else if dy > 0.0 {
            Dir::Down
        } else {
            Dir::Up
        };
        self.anchor = p;
        if self.dirs.last() == Some(&d) {
            return false;
        }
        self.dirs.push(d);
        true
    }

    /// 動かしている間に出す文字。「↑↓  再読み込み」のように、割り当てがあれば名前も付ける
    pub fn text(&self) -> Option<String> {
        if self.dirs.is_empty() {
            return None;
        }
        let arrows: String = self.dirs.iter().map(|d| d.arrow()).collect();
        Some(match command(&self.dirs) {
            Some(c) => format!("{arrows}  {}", c.label()),
            None => arrows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(points: &[(f32, f32)]) -> Tracker {
        let mut t = Tracker::new((100.0, 100.0), 30.0);
        for &p in points {
            t.moved(p);
        }
        t
    }

    #[test]
    fn small_moves_are_ignored() {
        let t = track(&[(110.0, 90.0), (120.0, 80.0)]);
        assert!(t.dirs.is_empty());
        assert_eq!(t.text(), None);
    }

    #[test]
    fn up_is_fullscreen() {
        let t = track(&[(100.0, 80.0), (102.0, 60.0), (101.0, 20.0)]);
        assert_eq!(t.dirs, [Dir::Up]);
        assert_eq!(command(&t.dirs), Some(Command::ToggleFullscreen));
        assert_eq!(t.text().as_deref(), Some("↑  全画面表示"));
    }

    #[test]
    fn up_down_is_reload() {
        // 上へ行ってから、下へ戻る（行き過ぎても同じ向きは 1 つにまとめる）
        let t = track(&[(100.0, 60.0), (100.0, 20.0), (100.0, 60.0), (100.0, 120.0), (100.0, 200.0)]);
        assert_eq!(t.dirs, [Dir::Up, Dir::Down]);
        assert_eq!(command(&t.dirs), Some(Command::Reload));
    }

    #[test]
    fn diagonal_uses_larger_axis_and_unknown_does_nothing() {
        let t = track(&[(140.0, 80.0)]);
        assert_eq!(t.dirs, [Dir::Right]);
        assert_eq!(command(&t.dirs), None);
        assert_eq!(t.text().as_deref(), Some("→"));
        let t = track(&[(100.0, 60.0), (100.0, 100.0), (100.0, 60.0)]);
        assert_eq!(t.dirs, [Dir::Up, Dir::Down, Dir::Up]);
        assert_eq!(command(&t.dirs), None);
    }
}
