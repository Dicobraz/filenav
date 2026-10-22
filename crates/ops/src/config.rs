//! Секция `[ops]` конфига (S5 §2.4).

use std::collections::BTreeMap;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use crate::api::OpOptions;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpsConfig {
    pub defaults: OpOptions,
    /// F8/Del — в корзину, Shift — насовсем (S0-decisions №11).
    pub delete_to_trash: bool,
    /// Общий потолок воркеров всех job.
    pub max_threads: u16,
    /// Одновременных job на устройство по классу носителя.
    pub device_jobs: DeviceJobs,
    /// Переопределения таблицы стратегий (S5-19): `"ssd>ssd" = "stream"`.
    pub strategy: BTreeMap<CompactString, CompactString>,
    /// Порог «мелкого» файла, байт.
    pub small_file: u32,
    /// Порог «крупного» файла (небуферизованный поток, диапазоны), байт.
    pub large_file: u64,
    /// Быстрая корзина (S5-26, «Решения для владельца» №5).
    pub experimental_fast_trash: bool,
}

impl Default for OpsConfig {
    fn default() -> Self {
        OpsConfig {
            defaults: OpOptions::default(),
            delete_to_trash: true,
            max_threads: 64,
            device_jobs: DeviceJobs::default(),
            strategy: BTreeMap::new(),
            small_file: 1 << 20,
            large_file: crate::strategy::LARGE_FILE,
            // S0-decisions №79: быстрая корзина по умолчанию включена.
            experimental_fast_trash: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceJobs {
    pub hdd: u8,
    pub ssd: u8,
    pub net: u8,
    pub usb: u8,
}

impl Default for DeviceJobs {
    fn default() -> Self {
        DeviceJobs {
            hdd: 1,
            ssd: 3,
            net: 2,
            usb: 1,
        }
    }
}
