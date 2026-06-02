use std::collections::VecDeque;
use std::time::Duration;

use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::StdRng;

use mycela::config::{WidgetConfig, WidgetType};
use mycela::widgets::collect_data_widgets;

/// Per-chart simulation state. The simulator only drives chart array PVs;
/// scalar PVs (double, int32, etc.) are left for external EPICS clients.
enum SimPv {
    /// Double array PV — rolling sine wave + noise buffer (line / histogram).
    DoubleArray {
        pv_name: String,
        buffer: VecDeque<f64>,
        max_points: usize,
        phase: f64,
        amplitude: f64,
        offset: f64,
    },
    /// Double array PV storing interleaved [x0,y0, x1,y1, ...] for scatter charts.
    ScatterArray {
        pv_name: String,
        buffer_x: VecDeque<f64>,
        buffer_y: VecDeque<f64>,
        max_points: usize,
        phase_x: f64,
        phase_y: f64,
        amplitude: f64,
        offset: f64,
    },
}

/// Spawn a background task that periodically generates random data and posts it
/// to the PVXS server via `ServerHandle`.  Only call this when the embedded
/// demo server is active.
pub fn start_demo_simulator(
    handle: pvxs_sys::ServerHandle,
    widgets: &[WidgetConfig],
) {
    let sim_pvs = build_sim_pvs(widgets);
    if sim_pvs.is_empty() {
        tracing::info!("Demo simulator: no server-backed PVs found, skipping");
        return;
    }

    tracing::info!(
        "Demo simulator: starting with {} simulated PVs",
        sim_pvs.len()
    );

    tokio::spawn(async move {
        run_simulation_loop(handle, sim_pvs).await;
    });
}

/// Scan the widget tree for PVs that have a `server` config block and build
/// the appropriate simulation state for each.
fn build_sim_pvs(widgets: &[WidgetConfig]) -> Vec<SimPv> {
    let data_widgets = collect_data_widgets(widgets);
    let mut sim_pvs = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for w in &data_widgets {
        let epics = match w.epics_pva() {
            Some(e) if e.server.is_some() => e,
            _ => continue,
        };
        let pv_name = epics.pv_name.clone();
        if !seen.insert(pv_name.clone()) {
            continue;
        }

        if w.widget_type == WidgetType::Group {
            continue;
        }

        let (low, high) = display_limits(w);

        match w.data_type.as_deref() {
            Some("double_array") => {
                let max_points = w.max_points.unwrap_or(100);
                let chart_type = w.chart_type.as_deref().unwrap_or("line");
                match chart_type {
                    "scatter" | "scatter_histogram" => {
                        sim_pvs.push(SimPv::ScatterArray {
                            pv_name: pv_name.clone(),
                            buffer_x: VecDeque::from(vec![0.0; max_points]),
                            buffer_y: VecDeque::from(vec![0.0; max_points]),
                            max_points,
                            phase_x: 0.0,
                            phase_y: std::f64::consts::FRAC_PI_2,
                            amplitude: (high - low) * 0.35,
                            offset: (high + low) / 2.0,
                        });
                    }
                    _ => {
                        sim_pvs.push(SimPv::DoubleArray {
                            pv_name: pv_name.clone(),
                            buffer: VecDeque::from(vec![0.0; max_points]),
                            max_points,
                            phase: 0.0,
                            amplitude: (high - low) * 0.3,
                            offset: (high + low) / 2.0,
                        });
                    }
                }
            }
            _ => {}
        }

        if w.chart_type.as_deref().unwrap_or("line") == "line" {
            if let Some(extra_pvs) = epics.pv_names.as_ref() {
                let max_points = w.max_points.unwrap_or(100);
                for (i, extra_name) in extra_pvs.iter().take(5).enumerate() {
                    if !seen.insert(extra_name.clone()) {
                        continue;
                    }
                    sim_pvs.push(SimPv::DoubleArray {
                        pv_name: extra_name.clone(),
                        buffer: VecDeque::from(vec![0.0; max_points]),
                        max_points,
                        phase: (i + 1) as f64 * std::f64::consts::FRAC_PI_3,
                        amplitude: (high - low) * 0.25,
                        offset: (high + low) / 2.0,
                    });
                }
            }
        }
    }

    sim_pvs
}

fn display_limits(w: &WidgetConfig) -> (f64, f64) {
    if let Some(epics) = w.epics_pva() {
        if let Some(server) = &epics.server {
            if let Some(meta) = &server.metadata {
                if let Some(display) = &meta.display {
                    return (display.limit_low, display.limit_high);
                }
            }
        }
    }
    (0.0, 100.0)
}

async fn run_simulation_loop(
    handle: pvxs_sys::ServerHandle,
    mut sim_pvs: Vec<SimPv>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(42);
    let mut rng = StdRng::seed_from_u64(seed);

    loop {
        interval.tick().await;

        let mut should_stop = false;

        for pv in sim_pvs.iter_mut() {
            match pv {
                SimPv::DoubleArray {
                    pv_name, buffer, max_points, phase, amplitude, offset,
                } => {
                    let noise = rng.random_range(-*amplitude * 0.15..=*amplitude * 0.15);
                    let y = *offset + *amplitude * phase.sin() + noise;
                    *phase += 0.15;

                    if buffer.len() >= *max_points { buffer.pop_front(); }
                    buffer.push_back(y);

                    let data: Vec<f64> = buffer.iter().copied().collect();
                    if let Err(e) = handle.post_double_array(pv_name, data) {
                        tracing::warn!("Simulator: failed to post {}: {}", pv_name, e);
                        should_stop = true;
                        break;
                    }
                }
                SimPv::ScatterArray {
                    pv_name, buffer_x, buffer_y, max_points,
                    phase_x, phase_y, amplitude, offset,
                } => {
                    let noise_x = rng.random_range(-*amplitude * 0.08..=*amplitude * 0.08);
                    let noise_y = rng.random_range(-*amplitude * 0.12..=*amplitude * 0.12);
                    let x = *offset + *amplitude * phase_x.sin() + noise_x;
                    let y = *offset + *amplitude * phase_y.sin() + noise_y;
                    *phase_x += 0.11;
                    *phase_y += 0.17;

                    if buffer_x.len() >= *max_points { buffer_x.pop_front(); }
                    if buffer_y.len() >= *max_points { buffer_y.pop_front(); }
                    buffer_x.push_back(x);
                    buffer_y.push_back(y);

                    let data: Vec<f64> = buffer_x.iter().zip(buffer_y.iter())
                        .flat_map(|(&xi, &yi)| [xi, yi])
                        .collect();
                    if let Err(e) = handle.post_double_array(pv_name, data) {
                        tracing::warn!("Simulator: failed to post {}: {}", pv_name, e);
                        should_stop = true;
                        break;
                    }
                }
            }
        }

        if should_stop {
            tracing::info!("Demo simulator: stopping after terminal post failure");
            break;
        }
    }
}
