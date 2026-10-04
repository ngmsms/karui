<img src="assets/karui.png" width="96" alt="karui icon">

# karui

Stupidly lightweight image viewer.

`karui` is a tiny, fast, Windows-only image viewer that keeps only the operations you actually use.  
It is distributed as a single executable and requires no installer.

Built with Claude Code.

## Highlights

- Single executable
  - x64: about 380 KB
  - ARM64: about 350 KB
- No installer required
- No Visual C++ redistributable required
  - The C runtime is embedded into the executable via `.cargo/config.toml`
- Fast startup
  - Usually around 0.1 seconds
- Low memory usage
  - Base overhead is around 15 MB
  - Beyond that, only the current image and one image before and after are kept in memory, at 4 bytes per pixel
    (for 24-megapixel photos, that is about 96 MB per image, or around 350 MB in total)
- Direct2D rendering
  - No .NET
  - No WPF
- NeeView-inspired controls
  - Left button + wheel to cycle display sizes, right-to-left page order, right-drag up for fullscreen
  - See [Relationship to NeeView](#relationship-to-neeview)

## Download

Download `karui-x64.exe` (or `karui-arm64.exe` for Windows on ARM) from [Releases](https://github.com/ngmsms/karui/releases) and run it. No installation is needed.
`SHA256SUMS.txt` lists the checksums of the executables.

The executables are not code-signed, so Windows SmartScreen may show "Windows protected your PC" the first time.
Click **More info** → **Run anyway**.

## Usage

Open an image or folder:

```
karui.exe <image-file-or-folder>
```

You can also drag and drop an image or folder onto the window.

Images in the same folder are shown in Explorer-like name order, with numeric parts compared numerically.

To associate `karui` with image files:

1. Right-click an image file
2. Choose **Open with**
3. Choose **Choose another app**
4. Select **Look for another app on this PC**
5. Select `karui.exe`

The title bar follows the Windows app mode (Settings → Personalization → Colors), light or dark, and updates if you switch it while `karui` is running.

## Controls

| Input | Action |
|---|---|
| Wheel up / down | If the image is larger than the window, scroll the image. If already at an edge, go to the previous / next page. If the image fits, go to the previous / next page. |
| Left button + wheel down / up | Cycle display size: original size → fit window → fit height → fit width. Wheel up reverses the order. Same as NeeView's default. |
| Left drag | Scroll an image that is larger than the window. |
| Click / drag the slider at the bottom | Jump to the selected page. By default, the right end is the first page and the left end is the last page. |
| Gear icon at the right end of the slider | Open the settings panel. Close it with `Esc` or by clicking outside. |
| Right-drag up (mouse gesture) | Toggle fullscreen. Same as NeeView's default. |
| Right-drag up, then down (mouse gesture) | Reload. Re-scan the folder and reload the image. |
| ← / → | Next / previous page. Follows NeeView's default right-to-left reading order. |
| Home / End | First / last page. |
| F11 | Toggle fullscreen. `Esc` also exits fullscreen. |

## Relationship to NeeView

`karui` is an independent, unofficial viewer. It is not affiliated with [NeeView](https://github.com/neelabo/NeeView) or its author.
It does not contain any NeeView code; several defaults were taken from NeeView's settings so that the controls feel familiar.

### Follows NeeView's defaults

- Left button + wheel cycles the display size (NeeView's "toggle stretch mode" shortcut), using the same order and Japanese display names
  (NeeView's "fill window" and "fit area" modes are left out)
- Right-to-left reading order: `←` goes to the next page
- An oversized image starts at the top-right when moving forward, and at the bottom-left when moving backward
- Images smaller than the window are also enlarged in fit modes
- Right-drag up toggles fullscreen; gestures are recognized every 30 DIP of movement, and the gesture in progress is shown in the center of the screen
- Wheel scrolling of an image that overflows both vertically and horizontally follows an N-shape
  (down to the bottom, shift one screen left, then down again from the top); distances under 10 DIP count as being at an edge
- The page slider runs right to left, shows the page number at the right end, is 25 DIP high,
  and in fullscreen stays hidden until the mouse reaches the bottom edge
- The mouse cursor is hidden after 2 seconds of inactivity

### karui's own behavior

- The wheel scrolls an oversized image, and moves to the next / previous page only once an edge has been reached
  (NeeView's wheel scrolling does not turn pages)
- The scroll amount is a percentage of the window, adjustable in the settings panel
- Wheel input is ignored while the next image is loading, so fast scrolling does not skip pages you have not seen
- Right-drag up, then down reloads the folder and the images; the current image stays visible until the reload finishes
- Settings are changed in a dark panel drawn inside the window, opened from the gear icon

## Settings

The settings panel can be opened from the gear icon.

| Setting | Default |
|---|---:|
| Scroll oversized images with the wheel | On |
| Scroll amount per wheel notch | 40% of the image area height, or width for horizontally overflowing images |
| Scroll animation time | 0.20 seconds |
| Ignore wheel input while loading | On |
| Page slider goes left to right | Off |

Settings are saved together with the display size and window position to:

```
%LOCALAPPDATA%\karui\settings.txt
```

The settings panel is drawn only while open, so it does not affect startup speed or memory usage.

## Supported formats

`karui` uses Windows Imaging Component, so it can read formats supported by WIC:

- JPEG
- PNG
- GIF, first frame only
- BMP
- TIFF
- ICO
- JPEG XR
- DDS

These formats may also work if the corresponding Microsoft Store codec extensions are installed:

- WebP
- HEIC
- AVIF
- JPEG XL

JPEG EXIF orientation is applied, so phone photos are displayed with the correct rotation.

Not supported:

- ZIP and other archives
- PDF
- Video
- Spread / two-page view

## Build

Requires Rust with the MSVC toolchain and Visual Studio Build Tools.

```
cargo build --release
```

The executable is created at:

```
target\release\karui.exe
```

Close `karui` before building. The build fails if the running executable cannot be overwritten.

### ARM64 (Windows on ARM)

```
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

The executable is created at:

```
target\aarch64-pc-windows-msvc\release\karui.exe
```

This also requires the ARM64 C++ build tools for Visual Studio Build Tools
("MSVC v143 - VS 2022 C++ ARM64/ARM64EC build tools", `Microsoft.VisualStudio.Component.VC.Tools.ARM64`).
Without them, linking fails. `tools\install-arm64-tools.ps1` adds them; it asks for administrator approval (UAC), so run it from a desktop session.

The x64 build also runs on Windows 11 on ARM through emulation, but the ARM64 build starts faster.

### Releases

Pushing a tag that starts with `v` runs `.github/workflows/release.yml` on GitHub Actions:
it runs the tests, builds the x64 and ARM64 executables, and attaches them to a new release.
The tag must match `version` in `Cargo.toml`.

```
git tag v0.1.1
git push origin v0.1.1
```

To redraw the icon, run `python tools/make-icon.py` (requires Pillow).

## License

MIT. See [LICENSE](LICENSE).
