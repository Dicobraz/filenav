# ADR-0012: рендер wgpu (DX12 на Windows) вместо glow

Статус: принято (владелец, 2026-10-16, при S1-13); уточнено в тот же день: на Windows основной DX12, а не Vulkan (см. «Уточнение»).

## Контекст

S1 §1/§3 и `00-ARCHITECTURE` выбрали glow (OpenGL), wgpu — «feature-флаг на будущее». На ручной приёмке S1-13 окно подвисало на 0,6–3 с при разворачивании и восстановлении.

Разбор на стенде (Windows 11, Intel Graphics, драйвер 32.0.101.8626):

- воспроизводится на **пустом** окне eframe 0.36 + glow без FileNav: 13 зависаний из 80 разворотов/восстановлений;
- сэмплер стека главного потока: 95 % времени — драйвер Intel OpenGL (`igxelpgicd64!DrvSwapBuffers` / `DrvReleaseContext`) крутит `SetEvent` на 100 % CPU;
- только во время анимации свёртывания/разворота DWM: `SetWindowPos` на тот же прямоугольник — 0 из 60;
- обходы на стороне glow не помогают: отключение анимации окна (`DWMWA_TRANSITIONS_FORCEDISABLED`) — 4 из 80, без vsync — так же, ранний выход в `change_gl_context` eframe — так же (ожидание переезжает в `SwapBuffers`).

Пустое окно eframe 0.36 на этом стенде:

| Рендер | Зависаний из 80 | Первый кадр, медиана | RAM private | Бинарь (fat LTO) |
|---|---|---|---|---|
| glow | 13 | 113 мс | 53 МБ | 5,6 МиБ |
| wgpu Vulkan, `MemoryHints::MemoryUsage` | 0 | 126 мс | 80 МБ | 10,0 МиБ |
| wgpu DX12, `MemoryHints::MemoryUsage` | — | 172 мс | 152 МБ | +0,9 МиБ к Vulkan |
| wgpu, подсказки по умолчанию | — | 174–202 мс | 260–330 МБ | — |

## Решение

- eframe с фичей `wgpu_no_default_features` (без `glow`); бэкенды wgpu по ОС: Windows — DX12, запасной Vulkan; Linux — Vulkan, запасной GLES; macOS — Metal. Следующий бэкенд поднимается, только если предыдущий не создал окна.
- Адаптер внутри бэкенда: встроенная графика (файловому менеджеру не нужна дискретная, и она не будится), программный рендер — последним.
- Устройство: `MemoryHints::MemoryUsage`, лимиты `downlevel_defaults` с текстурами до 8192.
- Present `AutoVsync`, перерисовка по-прежнему только по событию.
- Багрепорт в Intel о зависании OpenGL-драйвера — готовит app-сессия, отправляет владелец (`docs/bugreports/`).

## Уточнение: DX12 вместо Vulkan на Windows

На ручной приёмке — чёрный кадр при развороте. Замер (область экрана каждые ~14 мс, 20 разворотов `SC_MAXIMIZE`): Vulkan — чёрное 30–85 мс в **20 из 20** (при любом present mode: FIFO, Immediate, AutoNoVsync; задержка кадров 1 или 2), DX12 — **0 из 20**, одна чистая смена кадра; зависаний на DX12 — 0 из 60. Причина — пересоздание swapchain Vulkan при смене размера. Владелец выбрал DX12.

FileNav на этом стенде:

| Бэкенд | Чёрный кадр при развороте | Первый кадр, медиана | RAM private |
|---|---|---|---|
| Vulkan | 20 из 20, 30–85 мс | ≈ 129 мс | ≈ 88 МБ |
| DX12 | 0 из 20 | ≈ 154 мс | ≈ 162 МБ |

Что проверено для Vulkan, прежде чем уйти с него:

- `oldSwapchain` — wgpu-hal 30 уже передаёт старый swapchain в `vkCreateSwapchainKHR` (перед этим `vkDeviceWaitIdle`);
- кадр внутри `WM_SIZE` — eframe рисует сразу на `Resized` (`repaint_asap`), winit шлёт его из `WM_SIZE`;
- динамические viewport/scissor — egui-wgpu ставит их каждый кадр, пайплайны при ресайзе не пересоздаются;
- `VK_EXT_surface_maintenance1` + `VkSwapchainPresentScalingCreateInfoKHR` (растянуть старый кадр, пока готовится новый) — драйвер Intel (Vulkan 1.4.340) не поддерживает ни EXT-, ни KHR-версии `surface_maintenance1`/`swapchain_maintenance1`; в wgpu-hal их тоже нет;
- present mode FIFO / Immediate / AutoNoVsync и задержка кадров 1–2 — чёрный кадр остаётся.

Почти вся разница по RAM — драйвер Intel D3D12 (`igd12um64xeh`, компилятор шейдеров `igc64`); `MemoryHints::Manual` с мелкими блоками её не меняет.

## Последствия

- Нет зависаний при разворачивании на драйверах Intel OpenGL.
- RAM: ≈ 162 МБ на DX12 (Vulkan ≈ 88, glow ≈ 60); бюджет «RAM < 50 МБ» недостижим и на glow (пустое окно 53,9 МБ на этом драйвере), пересмотр — в S1-14. Первый кадр ≈ 154 мс — на 4 мс сверх бюджета 150, тоже S1-14.
- Бинарь +≈ 5 МиБ (Vulkan + DX12); бюджет 15 МиБ держится (`ci/size.py`).
- Машины без D3D12 на видеокарте работают через WARP (программный DX12) или Vulkan.
- S6 «план Б» терминала: `egui_wgpu::CallbackTrait` вместо `egui_glow::CallbackFn`; S9: RenderDoc работает с Vulkan/DX12.
- Код UI от бэкенда не зависит (как и задумано в `00-ARCHITECTURE`).
