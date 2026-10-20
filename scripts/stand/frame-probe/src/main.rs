//! Стенд Urfin: ловит «пустые» кадры. Desktop Duplication отдаёт каждый кадр, который DWM
//! вывел на экран; зонд вырезает прямоугольник окна процесса и в полосе списка (y 12…88 %)
//! считает пиксели цвета полосы курсора (серые `--colors`, по умолчанию 240 и 249).
//! Кадр, где их меньше порога, — кадр без курсора. Ввода не делает, только смотрит.
//!
//! `frame-probe --pid <PID> [--seconds 6] [--colors 240,249] [--min 500] [--out frames.csv]`

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput1, IDXGIResource,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, Interface};

struct Args {
    pid: u32,
    seconds: f64,
    colors: Vec<u8>,
    min: u32,
    out: Option<String>,
    /// Сохранить каждый N-й кадр как BMP (до 8 штук) рядом с `--out`.
    dump: u32,
}

fn args() -> Args {
    let mut a = Args {
        pid: 0,
        seconds: 6.0,
        colors: vec![240, 249],
        min: 500,
        out: None,
        dump: 0,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i + 1 < v.len() {
        let x = &v[i + 1];
        match v[i].as_str() {
            "--pid" => a.pid = x.parse().unwrap_or(0),
            "--seconds" => a.seconds = x.parse().unwrap_or(6.0),
            "--colors" => a.colors = x.split(',').filter_map(|c| c.parse().ok()).collect(),
            "--min" => a.min = x.parse().unwrap_or(500),
            "--out" => a.out = Some(x.clone()),
            "--dump" => a.dump = x.parse().unwrap_or(0),
            _ => {}
        }
        i += 2;
    }
    a
}

/// Видимое окно верхнего уровня процесса с самой большой площадью.
fn window_of(pid: u32) -> Option<(HWND, RECT)> {
    struct Acc {
        pid: u32,
        best: Option<(HWND, RECT)>,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` — наш `&mut Acc` на время EnumWindows.
        let acc = unsafe { &mut *(l.0 as *mut Acc) };
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
        if p == acc.pid && unsafe { IsWindowVisible(h) }.as_bool() {
            let mut r = RECT::default();
            let ok = unsafe {
                DwmGetWindowAttribute(
                    h,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    (&raw mut r).cast(),
                    size_of::<RECT>() as u32,
                )
            };
            let area = |r: &RECT| i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
            if ok.is_ok() && acc.best.is_none_or(|(_, b)| area(&r) > area(&b)) {
                acc.best = Some((h, r));
            }
        }
        BOOL(1)
    }
    let mut acc = Acc { pid, best: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&raw mut acc as isize));
    }
    acc.best
}

fn main() -> windows::core::Result<()> {
    let a = args();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let start = Instant::now();
    let (_, rect) = loop {
        if window_of(a.pid).is_some() {
            // Окно может ещё развернуться или сдвинуться: прямоугольник — после паузы.
            std::thread::sleep(Duration::from_millis(1500));
            if let Some(w) = window_of(a.pid) {
                break w;
            }
        }
        if start.elapsed() > Duration::from_secs(10) {
            eprintln!("окно процесса {} не найдено", a.pid);
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let cx = (rect.left + rect.right) / 2;
    let cy = (rect.top + rect.bottom) / 2;

    // Выход (монитор) с центром окна и его адаптер.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut found = None;
    let mut ai = 0;
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(ai) } {
        let mut oi = 0;
        while let Ok(out) = unsafe { adapter.EnumOutputs(oi) } {
            let d = unsafe { out.GetDesc()? };
            let r = d.DesktopCoordinates;
            if cx >= r.left && cx < r.right && cy >= r.top && cy < r.bottom {
                found = Some((adapter.clone(), out.cast::<IDXGIOutput1>()?, r));
            }
            oi += 1;
        }
        ai += 1;
    }
    let Some((adapter, output, desk)) = found else {
        eprintln!("монитор окна не найден");
        std::process::exit(2);
    };
    let (device, ctx) = device(&adapter)?;
    let dup = unsafe { output.DuplicateOutput(&device)? };

    // Окно в координатах монитора, обрезано по нему.
    let x0 = (rect.left - desk.left).max(0) as u32;
    let y0 = (rect.top - desk.top).max(0) as u32;
    let w = ((rect.right.min(desk.right) - rect.left.max(desk.left)).max(1)) as u32;
    let h = ((rect.bottom.min(desk.bottom) - rect.top.max(desk.top)).max(1)) as u32;
    let staging = staging(&device, w, h)?;
    eprintln!("окно {w}x{h} на мониторе в ({x0},{y0})");

    let mut rows = Vec::new();
    let mut frames = 0u32;
    let mut bad = 0u32;
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < a.seconds {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        match unsafe { dup.AcquireNextFrame(100, &mut info, &mut res) } {
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
            Err(e) => return Err(e),
            Ok(()) => {}
        }
        // Кадр с новым содержимым (не только движение мыши).
        if info.LastPresentTime != 0
            && let Some(res) = res
        {
            let tex: ID3D11Texture2D = res.cast()?;
            let src = windows::Win32::Graphics::Direct3D11::D3D11_BOX {
                left: x0,
                top: y0,
                front: 0,
                right: x0 + w,
                bottom: y0 + h,
                back: 1,
            };
            unsafe { ctx.CopySubresourceRegion(&staging, 0, 0, 0, 0, &tex, 0, Some(&src)) };
            let [l, r, ink] = count(&ctx, &staging, w, h)?;
            if a.dump > 0 && frames % a.dump == 0 && frames / a.dump < 8 {
                let path = format!("{}-{frames}.bmp", a.out.as_deref().unwrap_or("frame"));
                let _ = dump(&ctx, &staging, w, h, &path);
            }
            frames += 1;
            if l + r == 0 {
                bad += 1;
            }
            rows.push(format!("{:.1},{l},{r},{ink}", t0.elapsed().as_secs_f64() * 1000.0));
        }
        unsafe { dup.ReleaseFrame()? };
    }
    if let Some(out) = &a.out {
        let _ = std::fs::write(out, rows.join("\n"));
    }
    println!("frames={frames} without_cursor={bad} min={}", a.min);
    Ok(())
}

fn device(adapter: &IDXGIAdapter1) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut dev = None;
    let mut ctx = None;
    unsafe {
        D3D11CreateDevice(
            adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )?;
    }
    Ok((dev.expect("device"), ctx.expect("context")))
}

fn staging(dev: &ID3D11Device, w: u32, h: u32) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut t = None;
    unsafe { dev.CreateTexture2D(&desc, None, Some(&mut t))? };
    Ok(t.expect("texture"))
}

/// Полоса курсора любого цвета: строки пикселей (y 8…92 %), где ≥ 70 % проб одного цвета и
/// он не фон панели (самый частый цвет кадра) — отдельно в левой и правой половине окна.
/// Третье число — «чернила»: пробы не цвета фона (текст, иконки), чтобы видеть пустой список.
fn count(
    ctx: &ID3D11DeviceContext,
    t: &ID3D11Texture2D,
    w: u32,
    h: u32,
) -> windows::core::Result<[u32; 3]> {
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(t, 0, D3D11_MAP_READ, 0, Some(&mut m))? };
    let px = |x: u32, y: u32| -> u32 {
        // SAFETY: (x, y) внутри отображённой текстуры `w`×`h`.
        unsafe {
            *(m.pData.cast::<u8>().add((y * m.RowPitch + x * 4) as usize) as *const u32)
                & 0x00ff_ffff
        }
    };
    let (y0, y1) = (h * 8 / 100, h * 92 / 100);
    // Фон — самый частый цвет на сетке проб; на ней же «чернила».
    let mut freq = std::collections::HashMap::new();
    for y in (y0..y1).step_by(5) {
        for x in (0..w).step_by(7) {
            *freq.entry(px(x, y)).or_insert(0u32) += 1;
        }
    }
    let (bg, bg_n) = freq.iter().max_by_key(|e| e.1).map_or((0, 0), |e| (*e.0, *e.1));
    let ink = freq.values().sum::<u32>() - bg_n;
    let mut out = [0u32; 3];
    let halves = [(w / 40, w / 2 - w / 40), (w / 2 + w / 40, w - w / 40)];
    for (k, (from, to)) in halves.into_iter().enumerate() {
        for y in y0..y1 {
            let mut c = std::collections::HashMap::new();
            let n = 48u32;
            for i in 0..n {
                *c.entry(px(from + (to - from) * i / n, y)).or_insert(0u32) += 1;
            }
            let (col, cnt) = c.into_iter().max_by_key(|e| e.1).unwrap_or((0, 0));
            if col != bg && cnt * 10 >= n * 7 {
                out[k] += 1;
            }
        }
    }
    out[2] = ink;
    unsafe { ctx.Unmap(t, 0) };
    Ok(out)
}

/// Кадр в BMP (32 бит, сверху вниз) — посмотреть глазами.
fn dump(
    ctx: &ID3D11DeviceContext,
    t: &ID3D11Texture2D,
    w: u32,
    h: u32,
    path: &str,
) -> windows::core::Result<()> {
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(t, 0, D3D11_MAP_READ, 0, Some(&mut m))? };
    let mut f = Vec::with_capacity((54 + w * h * 4) as usize);
    let size = 54 + w * h * 4;
    f.extend_from_slice(b"BM");
    f.extend_from_slice(&size.to_le_bytes());
    f.extend_from_slice(&[0; 4]);
    f.extend_from_slice(&54u32.to_le_bytes());
    f.extend_from_slice(&40u32.to_le_bytes());
    f.extend_from_slice(&(w as i32).to_le_bytes());
    f.extend_from_slice(&(-(h as i32)).to_le_bytes());
    f.extend_from_slice(&1u16.to_le_bytes());
    f.extend_from_slice(&32u16.to_le_bytes());
    f.extend_from_slice(&[0; 24]);
    for y in 0..h {
        // SAFETY: строка `y` внутри отображённой текстуры.
        let row = unsafe {
            std::slice::from_raw_parts(
                m.pData.cast::<u8>().add((y * m.RowPitch) as usize),
                (w * 4) as usize,
            )
        };
        f.extend_from_slice(row);
    }
    unsafe { ctx.Unmap(t, 0) };
    let _ = std::fs::write(path, f);
    Ok(())
}
