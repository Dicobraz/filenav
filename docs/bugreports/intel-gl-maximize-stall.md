# Багрепорт в Intel: OpenGL-драйвер зависает при развороте окна

Черновик для владельца (ADR-0012). Отправляет владелец: Intel Community, раздел Graphics (нужен аккаунт Intel). Текст ниже — на английском, как принято на форуме; приложить `repro/main.rs`, `repro/Cargo.toml` и `stall.ps1` из этого файла.

---

**Title:** OpenGL ICD (igxelpgicd64) busy-spins 0.6–3 s in DrvSwapBuffers/DrvReleaseContext after window maximize/restore (Core Ultra 5 225H, 32.0.101.8626)

**Environment**

- CPU/GPU: Intel Core Ultra 5 225H, Intel(R) Graphics, PCI\VEN_8086&DEV_7DD1&SUBSYS_3F981043&REV_03
- Driver: 32.0.101.8626 (2026-03-11)
- OS: Windows 11 Pro 26H2, build 10.0.26300
- Display: single internal 2048×1152 (scaled), DWM composition, default settings

**Summary**

An OpenGL 3.3 core window (WGL via glutin/winit, double-buffered, swap interval 1 or 0) intermittently freezes for 0.6–3 seconds right after it is maximized or restored with `ShowWindow(SW_MAXIMIZE / SW_RESTORE)`. The UI thread is not waiting: it burns 100 % of one core inside the driver for the whole freeze.

**Reproduction**

1. Build the attached minimal app (Rust, eframe 0.36.2 with the glow backend — it just clears the window and draws one label).
2. Run `stall.ps1` — it maximizes and restores the window 40 times via `ShowWindow` and measures how long the window takes to answer `WM_NULL`.
3. Typical result: 13 of 80 operations take 0.6–3.0 s (normally < 20 ms).

**Observations**

- A stack sampler on the UI thread during the freeze (95 % of samples):
  `opengl32!wglSwapBuffers → gdi32full!SwapBuffers → igxelpgicd64!DrvSwapBuffers → … → KERNELBASE!SetEvent → ntdll!ZwSetEvent` (a tight loop), or the same loop under `opengl32!wglMakeCurrent(NULL) → igxelpgicd64!DrvReleaseContext`, depending on which call comes first after the resize.
- Only happens with the DWM minimize/maximize animation: resizing the same window to exactly the same maximized rectangle with `SetWindowPos` gives 0 freezes out of 60.
- Disabling the animation for the window (`DWMWA_TRANSITIONS_FORCEDISABLED`) reduces it (4 of 80) but does not remove it. Swap interval 0 or 1 makes no difference.
- The same app on the Vulkan driver (wgpu, Vulkan backend) and on D3D12 has no freezes (0 of 80).

**Expected:** SwapBuffers/MakeCurrent after a maximize/restore return within a frame, as with `SetWindowPos` resizes.

### repro/Cargo.toml

```toml
[package]
name = "gl-maximize-stall"
version = "0.1.0"
edition = "2024"

[dependencies]
eframe = { version = "=0.36.2", default-features = false, features = ["glow", "default_fonts"] }
```

### repro/main.rs

```rust
#![windows_subsystem = "windows"]
use eframe::egui;

struct App;
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("OpenGL maximize stall repro");
        });
    }
}

fn main() -> eframe::Result {
    let mut o = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 700.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    o.glow_options.vsync = true;
    eframe::run_native("gl-maximize-stall", o, Box::new(|_| Ok(Box::new(App))))
}
```

### stall.ps1

```powershell
param([string]$Exe = '.\target\release\gl-maximize-stall.exe', [int]$Rounds = 40)
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
}
'@
$p = Start-Process $Exe -PassThru
Start-Sleep -Milliseconds 1500
$p.Refresh(); $h = $p.MainWindowHandle
function Ping { $sw = [Diagnostics.Stopwatch]::StartNew(); $r = [IntPtr]::Zero
  [W]::SendMessageTimeout($h, 0, [IntPtr]::Zero, [IntPtr]::Zero, 0, 10000, [ref]$r) | Out-Null; $sw.ElapsedMilliseconds }
$slow = 0
for ($i = 0; $i -lt $Rounds; $i++) {
  [W]::ShowWindow($h, 3) | Out-Null; $a = Ping; Start-Sleep -Milliseconds 400   # SW_MAXIMIZE
  [W]::ShowWindow($h, 9) | Out-Null; $b = Ping; Start-Sleep -Milliseconds 400   # SW_RESTORE
  foreach ($ms in $a, $b) { if ($ms -gt 300) { $slow++; "stall: $ms ms" } }
}
"$slow of $($Rounds * 2) operations stalled > 300 ms"
$p.CloseMainWindow() | Out-Null
```
