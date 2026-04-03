use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex};

use sysinfo::{Components, Disks, Networks, ProcessesToUpdate, System};
use tokio::time::{Duration, Instant};

use crate::http::device::metrics::{CpuCoreMetrics, DeviceHealthIssue, DeviceMetrics, ProcessMemoryMetrics, TempReading};
use crate::http::storage;
use crate::ws::device::{CpuCoreSample, CpuTelemetry, DiskTelemetry, MemoryTelemetry, NetworkInterfaceSample, NetworkSample, TelemetrySample};

use super::config::process_metrics_cache_ttl;
use super::hardware::{read_cpu_temperature_c, read_cpu_throttle_status, sample_gpu};
use super::processes::collect_process_memory_metrics;
use super::storage_metrics::{collect_disk_metrics, collect_disk_partitions, filesystem_usage_for_path, select_disk_for_path};

#[derive(Clone)]
struct ProcessMetricsCacheEntry {
    fetched_at: Instant,
    metrics: Vec<ProcessMemoryMetrics>,
}

pub(super) struct SystemCollector {
    sys: System,
    disks: Disks,
    components: Components,
    networks: Networks,
    net_snapshot: Option<NetSnapshot>,
    process_metrics_cache: Option<ProcessMetricsCacheEntry>,
}

impl SystemCollector {
    pub(super) fn new() -> Self {
        let sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();

        networks.refresh(false);
        let mut collector = Self { sys, disks, components, networks, net_snapshot: None, process_metrics_cache: None };
        collector.update_net_snapshot();

        // Prime CPU stats so the first incremental sample is meaningful.
        collector.sys.refresh_cpu_all();

        collector
    }

    pub(super) fn collect_metrics(&mut self) -> DeviceMetrics {
        // Keep the sysinfo collector hot so incremental CPU samples stay meaningful.
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(false);
        self.components.refresh(false);

        let cpu_avg_pct = if self.sys.cpus().is_empty() { 0.0 } else { self.sys.global_cpu_usage() };
        let cpu_freq_mhz = self.sys.cpus().first().map(|cpu| cpu.frequency()).unwrap_or(0);
        let cpus = self.sys.cpus().iter().enumerate().map(|(idx, cpu)| CpuCoreMetrics { id: idx, name: cpu.name().to_string(), pct: cpu.cpu_usage(), freq_mhz: cpu.frequency() }).collect();
        let disks = collect_disk_metrics(&self.disks);
        let processes = self.collect_cached_process_memory_metrics();
        let temps = self.components.iter().filter_map(|component| component.temperature().map(|temperature_c| TempReading { label: component.label().to_string(), temperature_c })).collect();
        let storage_health = storage::probe_storage_health();
        let issues = storage_health.issues.into_iter().map(|issue| DeviceHealthIssue { code: issue.code.to_string(), description: issue.description }).collect::<Vec<_>>();
        let status = if issues.is_empty() { "healthy" } else { "degraded" }.to_string();

        DeviceMetrics {
            status,
            issues,
            cpu_avg_pct,
            cpu_freq_mhz,
            cpus,
            mem_total_bytes: self.sys.total_memory(),
            mem_used_bytes: self.sys.used_memory(),
            swap_total_bytes: self.sys.total_swap(),
            swap_used_bytes: self.sys.used_swap(),
            disks,
            processes,
            temps,
            api: None,
        }
    }

    fn collect_cached_process_memory_metrics(&mut self) -> Vec<ProcessMemoryMetrics> {
        let ttl = process_metrics_cache_ttl();
        if ttl > Duration::from_millis(0)
            && let Some(entry) = self.process_metrics_cache.as_ref()
            && entry.fetched_at.elapsed() < ttl
        {
            return entry.metrics.clone();
        }

        let metrics = collect_process_memory_metrics(&self.sys);
        if ttl > Duration::from_millis(0) {
            self.process_metrics_cache = Some(ProcessMetricsCacheEntry { fetched_at: Instant::now(), metrics: metrics.clone() });
        } else {
            self.process_metrics_cache = None;
        }
        metrics
    }

    pub(super) fn collect_telemetry(&mut self) -> TelemetrySample {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.disks.refresh(true);
        self.components.refresh(false);

        let cpu_usage = self.sys.global_cpu_usage();
        let cores = self
            .sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(idx, cpu)| CpuCoreSample { id: idx, usage_percent: cpu.cpu_usage(), frequency_mhz: Some(cpu.frequency()), label: Some(cpu.name().to_string()) })
            .collect();
        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let partitions = collect_disk_partitions(&self.disks);
        let (disk_total, disk_used, disk_free) = storage::data_root_path()
            .ok()
            .and_then(|data_root| {
                filesystem_usage_for_path(&data_root).or_else(|| {
                    select_disk_for_path(&self.disks, &data_root).map(|disk| {
                        let total = disk.total_space();
                        let free = disk.available_space();
                        (total, total.saturating_sub(free), free)
                    })
                })
            })
            .unwrap_or_else(|| {
                let total = partitions.iter().map(|partition| partition.total_bytes).sum();
                let used = partitions.iter().map(|partition| partition.used_bytes).sum();
                let free = partitions.iter().map(|partition| partition.free_bytes).sum();
                (total, used, free)
            });

        TelemetrySample {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            engine: None,
            cpu: CpuTelemetry { usage_percent: cpu_usage, temperature_c: read_cpu_temperature_c(), throttle: read_cpu_throttle_status(), cores },
            memory: MemoryTelemetry { total_bytes: total_mem, used_bytes: used_mem, free_bytes: total_mem.saturating_sub(used_mem) },
            gpu: sample_gpu(),
            disk: Some(DiskTelemetry { total_bytes: disk_total, used_bytes: disk_used, free_bytes: disk_free }),
            disks: partitions,
            power: None,
            network: self.build_network_sample(),
        }
    }

    fn update_net_snapshot(&mut self) {
        let mut totals = BTreeMap::new();
        for (name, data) in self.networks.iter() {
            totals.insert(name.to_string(), (data.total_received(), data.total_transmitted()));
        }
        self.net_snapshot = Some(NetSnapshot { at: Instant::now(), totals });
    }

    fn build_network_sample(&mut self) -> Option<NetworkSample> {
        let prev = self.net_snapshot.clone();
        let mut totals = BTreeMap::new();
        let mut interfaces = Vec::new();
        let snapshot_time = prev.as_ref().map(|snap| snap.at);

        for (name, data) in self.networks.iter() {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            totals.insert(name.to_string(), (rx, tx));
            let prev_vals = prev.as_ref().and_then(|snap| snap.totals.get(name)).copied();
            let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_vals, rx, tx, snapshot_time);
            interfaces.push(NetworkInterfaceSample {
                name: name.to_string(),
                rx_bytes: delta_rx,
                tx_bytes: delta_tx,
                total_rx_bytes: rx,
                total_tx_bytes: tx,
                rx_bytes_per_sec: per_sec_rx,
                tx_bytes_per_sec: per_sec_tx,
                mac: None,
            });
        }

        let total_rx = totals.values().map(|(rx, _)| *rx).sum();
        let total_tx = totals.values().map(|(_, tx)| *tx).sum();
        let prev_totals = prev.as_ref().map(|snap| {
            let rx_sum: u64 = snap.totals.values().map(|(rx, _)| *rx).sum();
            let tx_sum: u64 = snap.totals.values().map(|(_, tx)| *tx).sum();
            (rx_sum, tx_sum)
        });
        let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_totals, total_rx, total_tx, snapshot_time);

        self.update_net_snapshot();

        if total_rx == 0 && total_tx == 0 && interfaces.is_empty() {
            return None;
        }

        Some(NetworkSample { rx_bytes: delta_rx, tx_bytes: delta_tx, total_rx_bytes: total_rx, total_tx_bytes: total_tx, rx_bytes_per_sec: per_sec_rx, tx_bytes_per_sec: per_sec_tx, interfaces })
    }
}

pub(super) fn collect_device_metrics(collector: &Arc<StdMutex<SystemCollector>>) -> DeviceMetrics {
    collector.lock().expect("system collector poisoned").collect_metrics()
}

#[derive(Clone)]
struct NetSnapshot {
    at: Instant,
    totals: BTreeMap<String, (u64, u64)>,
}

fn compute_net_deltas(prev: Option<(u64, u64)>, rx: u64, tx: u64, snapshot_time: Option<Instant>) -> (u64, u64, f64, f64) {
    if let (Some((prev_rx, prev_tx)), Some(at)) = (prev, snapshot_time) {
        let elapsed = at.elapsed().as_secs_f64().max(0.001);
        let delta_rx = rx.saturating_sub(prev_rx);
        let delta_tx = tx.saturating_sub(prev_tx);
        let per_sec_rx = delta_rx as f64 / elapsed;
        let per_sec_tx = delta_tx as f64 / elapsed;
        (delta_rx, delta_tx, per_sec_rx, per_sec_tx)
    } else {
        (0, 0, 0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn compute_net_deltas_returns_zero_without_previous_snapshot() {
        let (rx, tx, rx_per_sec, tx_per_sec) = compute_net_deltas(None, 100, 200, None);
        assert_eq!((rx, tx), (0, 0));
        assert_eq!((rx_per_sec, tx_per_sec), (0.0, 0.0));
    }

    #[test]
    fn compute_net_deltas_reports_positive_rates_with_previous_snapshot() {
        let then = Instant::now();
        thread::sleep(Duration::from_millis(2));
        let (rx, tx, rx_per_sec, tx_per_sec) = compute_net_deltas(Some((100, 200)), 160, 260, Some(then));
        assert_eq!((rx, tx), (60, 60));
        assert!(rx_per_sec > 0.0);
        assert!(tx_per_sec > 0.0);
    }
}
