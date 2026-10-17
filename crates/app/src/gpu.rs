//! Выбор рендера (ADR-0012, S0-decisions №50): `[ui] renderer`, очередь попыток,
//! откат на WARP при отказе DX12 и при потере устройства.
//!
//! Windows: `auto` и `dx12` — аппаратный DX12, при отказе — WARP (программный DX12,
//! «Microsoft Basic Render Driver»); `warp` — сразу WARP; `vulkan` — только явным
//! выбором (чёрный кадр при развороте на Intel), при отказе — WARP. Linux и macOS
//! настройку не читают: их бэкенды — по порядку, любой адаптер.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

/// Одна попытка поднять рендер.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gpu {
    /// Аппаратный адаптер бэкенда; программный (Cpu) не берётся — для него WARP.
    Hardware(wgpu::Backends),
    /// WARP — программный DX12.
    #[cfg_attr(not(windows), allow(dead_code, reason = "WARP есть только в Windows"))]
    Warp,
    /// Любой адаптер бэкенда: `WGPU_BACKEND` для диагностики, Linux и macOS.
    Any(wgpu::Backends),
}

impl Gpu {
    pub fn backends(self) -> wgpu::Backends {
        match self {
            Gpu::Hardware(b) | Gpu::Any(b) => b,
            Gpu::Warp => wgpu::Backends::DX12,
        }
    }

    fn accepts(self, kind: wgpu::DeviceType) -> bool {
        match self {
            Gpu::Hardware(_) => kind != wgpu::DeviceType::Cpu,
            Gpu::Warp => kind == wgpu::DeviceType::Cpu,
            Gpu::Any(_) => true,
        }
    }

    /// Прозрачный swapchain для Mica — только DX12 (`DxgiFromVisual`).
    pub fn mica_capable(self) -> bool {
        self.backends() == wgpu::Backends::DX12
    }
}

/// Очередь попыток по `[ui] renderer`; вторым — предупреждение о неизвестном значении.
#[cfg(windows)]
pub fn plan(renderer: &str) -> (Vec<Gpu>, Option<String>) {
    use wgpu::Backends as B;
    let dx12 = vec![Gpu::Hardware(B::DX12), Gpu::Warp];
    match renderer {
        "auto" | "dx12" => (dx12, None),
        "warp" => (vec![Gpu::Warp], None),
        "vulkan" => (vec![Gpu::Hardware(B::VULKAN), Gpu::Warp], None),
        other => (
            dx12,
            Some(format!(
                "expected auto | dx12 | warp | vulkan, got {other:?}"
            )),
        ),
    }
}

/// Не Windows: бэкенды по порядку, `[ui] renderer` не читается (docs/macos-notes.md).
#[cfg(not(windows))]
pub fn plan(_renderer: &str) -> (Vec<Gpu>, Option<String>) {
    #[cfg(target_os = "macos")]
    let order = [wgpu::Backends::METAL];
    #[cfg(not(target_os = "macos"))]
    let order = [wgpu::Backends::VULKAN, wgpu::Backends::GL];
    (order.into_iter().map(Gpu::Any).collect(), None)
}

/// Попытки после отказа `failed` (потеря устройства): остаток очереди.
pub fn after(plan: &[Gpu], failed: Gpu) -> Vec<Gpu> {
    plan.iter()
        .skip_while(|&&g| g != failed)
        .skip(1)
        .copied()
        .collect()
}

/// Порядок адаптеров одного бэкенда (меньше — лучше): встроенная графика —
/// дискретная файловому менеджеру не нужна; программный рендер — последним.
pub fn adapter_rank(device: wgpu::DeviceType) -> u8 {
    use wgpu::DeviceType as D;
    match device {
        D::IntegratedGpu => 0,
        D::DiscreteGpu => 1,
        D::VirtualGpu => 2,
        D::Other => 3,
        D::Cpu => 4,
    }
}

fn pick_adapter(
    gpu: Gpu,
    adapters: &[wgpu::Adapter],
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    #[cfg(debug_assertions)]
    if gpu == Gpu::Hardware(wgpu::Backends::DX12)
        && std::env::var_os("URFIN_FORCE_DX12_FAIL").is_some()
    {
        return Err("URFIN_FORCE_DX12_FAIL: hardware DX12 refused".to_owned());
    }
    let best = adapters
        .iter()
        .filter(|a| gpu.accepts(a.get_info().device_type))
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| adapter_rank(a.get_info().device_type))
        .cloned()
        .ok_or_else(|| format!("{gpu:?}: no adapter can draw to the window"))?;
    let i = best.get_info();
    tracing::info!(backend = ?i.backend, device = %i.name, kind = ?i.device_type, driver = %i.driver_info, "graphics adapter");
    crate::selftest::note_adapter(format!(
        "{:?} {} ({:?}, driver {})",
        i.backend, i.name, i.device_type, i.driver_info
    ));
    Ok(best)
}

/// wgpu по ADR-0012: экономный по памяти режим, адаптер — [`adapter_rank`] среди
/// подходящих попытке `gpu`. Present — `AutoVsync` (по умолчанию egui-wgpu).
/// `mica` — DX12 через `DirectComposition` (`DxgiFromVisual`): swapchain с альфой, сквозь
/// прозрачные пиксели виден Mica (S2 §1.4). Без Mica — обычный swapchain окна.
pub fn wgpu_options(gpu: Gpu, mica: bool) -> WgpuConfiguration {
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = gpu.backends();
    if mica {
        setup
            .instance_descriptor
            .backend_options
            .dx12
            .presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
        pick_adapter(gpu, adapters, surface)
    }));
    setup.device_descriptor = Arc::new(|_| wgpu::DeviceDescriptor {
        label: Some("urfin"),
        required_limits: wgpu::Limits {
            // Текстура на весь экран — до 8K.
            max_texture_dimension_2d: 8192,
            ..wgpu::Limits::downlevel_defaults()
        },
        // Блоки памяти по умолчанию (Performance) — +200 МБ RAM на встроенной
        // графике, где видеопамять — это RAM процесса.
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        ..Default::default()
    });
    let default = WgpuConfiguration::default();
    let on_status = default.on_surface_status.clone();
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        // Статусы поверхности (resize, occluded, lost) — в самопроверку; действие —
        // как у egui-wgpu.
        on_surface_status: Arc::new(move |status| {
            let name = format!("{status:?}");
            crate::selftest::note_surface(name.split(['(', ' ']).next().unwrap_or(&name));
            on_status(status)
        }),
        ..default
    }
}

/// Потеря устройства (сбой драйвера, TDR): флаг для кадра и перерисовка, чтобы
/// окно перезапустилось на следующей попытке. Ошибки wgpu без потери устройства —
/// в лог (в debug — паника, как обработчик wgpu по умолчанию).
pub fn watch_device(device: &wgpu::Device, ctx: &eframe::egui::Context, lost: Arc<AtomicBool>) {
    let flag = Arc::clone(&lost);
    let repaint = ctx.clone();
    device.set_device_lost_callback(move |reason, msg| {
        // `Destroyed` — штатное освобождение устройства при закрытии окна.
        if reason == wgpu::DeviceLostReason::Destroyed {
            return;
        }
        tracing::error!(?reason, %msg, "graphics device lost");
        flag.store(true, Ordering::Release);
        repaint.request_repaint();
    });
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        if lost.load(Ordering::Acquire) {
            tracing::debug!(error = %e, "wgpu error after device loss");
            return;
        }
        tracing::error!(error = %e, "wgpu error");
        debug_assert!(false, "wgpu error: {e}");
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_order() {
        use wgpu::DeviceType as D;
        let mut v = [
            D::Cpu,
            D::DiscreteGpu,
            D::Other,
            D::IntegratedGpu,
            D::VirtualGpu,
        ];
        v.sort_by_key(|&d| adapter_rank(d));
        assert_eq!(
            v,
            [
                D::IntegratedGpu,
                D::DiscreteGpu,
                D::VirtualGpu,
                D::Other,
                D::Cpu
            ]
        );
    }

    #[test]
    fn hardware_never_takes_software_and_warp_only_software() {
        use wgpu::DeviceType as D;
        let hw = Gpu::Hardware(wgpu::Backends::DX12);
        assert!(hw.accepts(D::IntegratedGpu) && hw.accepts(D::DiscreteGpu));
        assert!(!hw.accepts(D::Cpu));
        assert!(Gpu::Warp.accepts(D::Cpu) && !Gpu::Warp.accepts(D::IntegratedGpu));
        assert!(Gpu::Any(wgpu::Backends::GL).accepts(D::Cpu));
    }

    #[cfg(windows)]
    #[test]
    fn renderer_setting_plans() {
        use wgpu::Backends as B;
        let dx12 = vec![Gpu::Hardware(B::DX12), Gpu::Warp];
        assert_eq!(plan("auto"), (dx12.clone(), None));
        assert_eq!(plan("dx12"), (dx12.clone(), None));
        assert_eq!(plan("warp"), (vec![Gpu::Warp], None));
        assert_eq!(
            plan("vulkan"),
            (vec![Gpu::Hardware(B::VULKAN), Gpu::Warp], None)
        );
        let (p, w) = plan("opengl");
        assert_eq!(p, dx12);
        assert!(w.is_some_and(|w| w.contains("opengl")));
    }

    #[test]
    fn after_loss_goes_to_the_rest_of_the_plan() {
        let p = [Gpu::Hardware(wgpu::Backends::DX12), Gpu::Warp];
        assert_eq!(after(&p, p[0]), [Gpu::Warp]);
        assert_eq!(after(&p, Gpu::Warp), Vec::<Gpu>::new());
    }
}
