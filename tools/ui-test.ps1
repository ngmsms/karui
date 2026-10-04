# karui を起動して操作を送り、描画結果を BMP で残す検証スクリプト。
# リモート接続が切れた（画面キャプチャできない）状態でも使える。
#
# 準備:  cargo build --release --features snapshot --target-dir target/snap
# 実行:  pwsh -File tools\ui-test.ps1 -Image <画像> -Out <出力先> -Steps "shot:a;lwheel:-120;shot:b"
#
# Steps は ; 区切り:
#   shot:<名前>     描画結果を <出力先>\NN_<名前>.bmp に保存（タイトルと 1 フレームの描画時間も表示）
#   wheel:<delta>   ホイール（-120 で 1 ノッチ下 = 次のページ）
#   lwheel:<delta>  左ボタンを押しながらホイール（表示サイズの切り替え）
#   drag:<dx>:<dy>  左ドラッグ（ウィンドウ中央付近から）
#   click:<x>:<y>   左クリック。y が負なら下端から、x が負なら右端から（-12:-12 で歯車）
#   move:<x>:<y>    マウス移動（ボタンなし）。y の扱いは click と同じ
#   sdrag:<x1>:<y>:<x2>  (x1, y) から (x2, y) まで左ドラッグ
#   wheeluntil:<delta>:<max>  ページが変わるまでホイールを 1 ノッチずつ送り、回数を出す
#   rdown:<x>:<y> / rmove:<x>:<y> / rup  右ボタンを押す・押したまま動かす・離す（マウスジェスチャー）
#   run:<ps1 のパス>  スクリプトを実行する（途中でファイルを変えるときなど）
#   key:<vk>        キー（37=←, 39=→, 36=Home, 35=End, 122=F11, 27=Esc）
#   wheelt:<delta> / keyt:<vk>  操作してから画像が表示されるまでの時間を測る
#   sleep:<ms>      待つ
#   mem             メモリ使用量を表示
param(
    [string]$Exe = (Join-Path $PSScriptRoot "..\target\snap\release\karui.exe"),
    [Parameter(Mandatory)][string]$Image,
    [Parameter(Mandatory)][string]$Out,
    [string]$Steps = "shot:open"
)
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class H {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public static int ClientH(IntPtr h) { RECT r; GetClientRect(h, out r); return r.B - r.T; }
    public static int ClientW(IntPtr h) { RECT r; GetClientRect(h, out r); return r.R - r.L; }
    public static string Title(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
    public static IntPtr LP(int x, int y) { return (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff)); }
}
"@
$WM_MOUSEWHEEL = 0x020A; $WM_LBUTTONDOWN = 0x0201; $WM_LBUTTONUP = 0x0202; $WM_MOUSEMOVE = 0x0200
$WM_KEYDOWN = 0x0100; $WM_CLOSE = 0x0010; $WM_APP_SNAPSHOT = 0x8002

New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter *.bmp | Remove-Item
$env:KARUI_SNAPSHOT_DIR = (Resolve-Path $Out).Path

$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath $Exe -ArgumentList "`"$Image`"" -PassThru
while ($p.MainWindowHandle -eq 0 -and $sw.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 5; $p.Refresh() }
$h = $p.MainWindowHandle
Write-Output ("window after {0} ms" -f $sw.ElapsedMilliseconds)
Start-Sleep -Milliseconds 400

# 操作したときに画像が出るまで（タイトルに倍率が付くまで）待つ
function Wait-Shown($before, $t0) {
    while ($t0.ElapsedMilliseconds -lt 3000) {
        $t = [H]::Title($h)
        if ($t -ne $before -and $t -match '%') { break }
        Start-Sleep -Milliseconds 1
    }
    Write-Output ("{0} ms -> {1}" -f $t0.ElapsedMilliseconds, $t)
}

$n = 0
$cx = 600; $cy = 400
foreach ($s in ($Steps -split ";")) {
    $parts = $s -split ':'
    switch ($parts[0]) {
        'shot' {
            $n++
            [H]::PostMessage($h, $WM_APP_SNAPSHOT, [IntPtr]$n, [IntPtr]0) | Out-Null
            Start-Sleep -Milliseconds 150
            $dst = Join-Path $Out ("{0:d2}_{1}.bmp" -f $n, $parts[1])
            Move-Item -Force (Join-Path $Out ("snap_{0:d2}.bmp" -f $n)) $dst
            $bench = Join-Path $Out ("snap_{0:d2}.txt" -f $n)
            $ms = if (Test-Path $bench) { Get-Content $bench; Remove-Item $bench } else { "?" }
            Write-Output ("shot {0}  title='{1}'  render={2}" -f (Split-Path $dst -Leaf), [H]::Title($h), $ms)
        }
        'wheel' {
            $w = [IntPtr]((([int]$parts[1]) -band 0xffff) -shl 16)
            [H]::PostMessage($h, $WM_MOUSEWHEEL, $w, [H]::LP($cx, $cy)) | Out-Null
        }
        'lwheel' {
            $w = [IntPtr](((([int]$parts[1]) -band 0xffff) -shl 16) -bor 1)
            [H]::PostMessage($h, $WM_MOUSEWHEEL, $w, [H]::LP($cx, $cy)) | Out-Null
        }
        'drag' {
            $dx = [int]$parts[1]; $dy = [int]$parts[2]
            [H]::PostMessage($h, $WM_LBUTTONDOWN, [IntPtr]1, [H]::LP($cx, $cy)) | Out-Null
            for ($i = 1; $i -le 10; $i++) {
                [H]::PostMessage($h, $WM_MOUSEMOVE, [IntPtr]1, [H]::LP($cx + $dx * $i / 10, $cy + $dy * $i / 10)) | Out-Null
            }
            [H]::PostMessage($h, $WM_LBUTTONUP, [IntPtr]0, [H]::LP($cx + $dx, $cy + $dy)) | Out-Null
        }
        'click' {
            $x = [int]$parts[1]; $y = [int]$parts[2]; if ($y -lt 0) { $y += [H]::ClientH($h) }; if ($x -lt 0) { $x += [H]::ClientW($h) }
            [H]::PostMessage($h, $WM_LBUTTONDOWN, [IntPtr]1, [H]::LP($x, $y)) | Out-Null
            [H]::PostMessage($h, $WM_LBUTTONUP, [IntPtr]0, [H]::LP($x, $y)) | Out-Null
        }
        'move' {
            $x = [int]$parts[1]; $y = [int]$parts[2]; if ($y -lt 0) { $y += [H]::ClientH($h) }
            [H]::PostMessage($h, $WM_MOUSEMOVE, [IntPtr]0, [H]::LP($x, $y)) | Out-Null
        }
        'sdrag' {
            $x1 = [int]$parts[1]; $y = [int]$parts[2]; $x2 = [int]$parts[3]; if ($y -lt 0) { $y += [H]::ClientH($h) }
            [H]::PostMessage($h, $WM_LBUTTONDOWN, [IntPtr]1, [H]::LP($x1, $y)) | Out-Null
            for ($i = 1; $i -le 10; $i++) {
                [H]::PostMessage($h, $WM_MOUSEMOVE, [IntPtr]1, [H]::LP($x1 + ($x2 - $x1) * $i / 10, $y)) | Out-Null
                Start-Sleep -Milliseconds 20
            }
            [H]::PostMessage($h, $WM_LBUTTONUP, [IntPtr]0, [H]::LP($x2, $y)) | Out-Null
        }
        'wheeluntil' {
            # ページが変わるまで 1 ノッチずつ送り、何ノッチかかったかを出す
            $before = [H]::Title($h); $n2 = 0
            $w = [IntPtr]((([int]$parts[1]) -band 0xffff) -shl 16)
            while ($n2 -lt [int]$parts[2]) {
                [H]::PostMessage($h, $WM_MOUSEWHEEL, $w, [H]::LP($cx, $cy)) | Out-Null
                $n2++
                Start-Sleep -Milliseconds 40
                $t = [H]::Title($h)
                if (($t -split '\[')[1] -ne ($before -split '\[')[1]) { break }
            }
            Write-Output ("{0} notches -> {1}" -f $n2, [H]::Title($h))
        }
        'rdown' {
            $rx = [int]$parts[1]; $ry = [int]$parts[2]
            [H]::PostMessage($h, 0x0204, [IntPtr]2, [H]::LP($rx, $ry)) | Out-Null
        }
        'rmove' {
            $x2 = [int]$parts[1]; $y2 = [int]$parts[2]
            for ($i = 1; $i -le 5; $i++) {
                [H]::PostMessage($h, $WM_MOUSEMOVE, [IntPtr]2, [H]::LP($rx + ($x2 - $rx) * $i / 5, $ry + ($y2 - $ry) * $i / 5)) | Out-Null
            }
            $rx = $x2; $ry = $y2
        }
        'title' { Write-Output ("title: {0}" -f [H]::Title($h)) }
        'rup' { [H]::PostMessage($h, 0x0205, [IntPtr]0, [H]::LP($rx, $ry)) | Out-Null }
        'run' { & ($parts[1..($parts.Length - 1)] -join ':') }
        'key' { [H]::PostMessage($h, $WM_KEYDOWN, [IntPtr][int]$parts[1], [IntPtr]0) | Out-Null }
        'wheelt' {
            $before = [H]::Title($h); $t0 = [Diagnostics.Stopwatch]::StartNew()
            $w = [IntPtr]((([int]$parts[1]) -band 0xffff) -shl 16)
            [H]::PostMessage($h, $WM_MOUSEWHEEL, $w, [H]::LP($cx, $cy)) | Out-Null
            Wait-Shown $before $t0
        }
        'keyt' {
            $before = [H]::Title($h); $t0 = [Diagnostics.Stopwatch]::StartNew()
            [H]::PostMessage($h, $WM_KEYDOWN, [IntPtr][int]$parts[1], [IntPtr]0) | Out-Null
            Wait-Shown $before $t0
        }
        'sleep' { Start-Sleep -Milliseconds ([int]$parts[1]) }
        'mem' {
            $p.Refresh()
            Write-Output ("mem: private={0:n0} KB  ws={1:n0} KB" -f ($p.PrivateMemorySize64 / 1KB), ($p.WorkingSet64 / 1KB))
        }
    }
    Start-Sleep -Milliseconds 250
}
[H]::PostMessage($h, $WM_CLOSE, [IntPtr]0, [IntPtr]0) | Out-Null
if (-not $p.WaitForExit(3000)) { Write-Output "did not exit, killing"; $p.Kill() } else { Write-Output "exited $($p.ExitCode)" }
