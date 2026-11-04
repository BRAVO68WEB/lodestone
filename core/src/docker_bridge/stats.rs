/// Share of the host, from 0 to 100. One core of four fully used is 25.
///
/// Returns `None` when the counters go backwards. A repeated sample is `Some(0)`.
pub fn docker_cpu_percent(
    cpu_total: u64,
    prev_cpu_total: u64,
    system_total: u64,
    prev_system_total: u64,
) -> Option<f32> {
    if cpu_total < prev_cpu_total || system_total < prev_system_total {
        return None;
    }
    let cpu_delta = (cpu_total - prev_cpu_total) as f64;
    let system_delta = (system_total - prev_system_total) as f64;
    if system_delta == 0.0 {
        return if cpu_delta == 0.0 { Some(0.0) } else { None };
    }
    Some((cpu_delta / system_delta * 100.0) as f32)
}

/// Bytes of memory that are not reclaimable file cache.
pub fn docker_memory_bytes(usage: Option<u64>, inactive_file: u64) -> Option<u64> {
    let usage = usage?;
    Some(usage.saturating_sub(inactive_file.min(usage)))
}

#[cfg(test)]
mod tests {
    use super::{docker_cpu_percent, docker_memory_bytes};

    #[test]
    fn one_core_of_four_is_a_quarter_of_the_host() {
        assert_eq!(docker_cpu_percent(1_500, 1_000, 12_000, 10_000), Some(25.0));
    }

    #[test]
    fn the_first_sample_has_no_previous_delta() {
        assert_eq!(docker_cpu_percent(1_000, 1_000, 10_000, 10_000), Some(0.0));
    }

    #[test]
    fn a_reset_counter_is_not_zero_cpu() {
        assert_eq!(docker_cpu_percent(10, 1_000, 12_000, 10_000), None);
    }

    #[test]
    fn memory_subtracts_inactive_file_cache() {
        assert_eq!(docker_memory_bytes(Some(1_000), 200), Some(800));
        assert_eq!(docker_memory_bytes(Some(100), 500), Some(0));
        assert_eq!(docker_memory_bytes(None, 10), None);
    }
}
