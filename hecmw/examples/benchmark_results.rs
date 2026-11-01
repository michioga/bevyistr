//! Read-only CPU timings for native ASCII RES and its matching MSH.
use fem_core::{FemMesh, ResultField, StepResult};
use hecmw::native_result::NativeResult;
use std::{hint::black_box, path::PathBuf, time::Instant};

fn median_ms<T>(mut run: impl FnMut() -> Result<T, String>) -> Result<f64, String> {
    let mut times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let value = run()?;
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        black_box(value); // destruction is outside the measured interval
    }
    times.sort_by(f64::total_cmp);
    Ok(times[2])
}

// Deterministic, non-cryptographic comparison of mesh order, metadata, field
// names/kinds, ranges and every stored value (including missing-value NaNs).
fn signature(mesh: &FemMesh, steps: &[StepResult]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |bytes: &[u8]| {
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    };
    for node in &mesh.nodes {
        add(&node.id.0.to_le_bytes());
    }
    for element in &mesh.elements {
        add(&element.id.0.to_le_bytes());
    }
    for step in steps {
        add(&step.step.to_le_bytes());
        add(&step.time.to_bits().to_le_bytes());
        match step.eigenvalue {
            Some(value) => {
                add(&[1]);
                add(&value.to_bits().to_le_bytes());
            }
            None => add(&[0]),
        }
        for field in &step.fields {
            add(&(field.name().len() as u64).to_le_bytes());
            add(field.name().as_bytes());
            match field {
                ResultField::NodeScalar {
                    values, min, max, ..
                }
                | ResultField::ElementScalar {
                    values, min, max, ..
                } => {
                    add(&[if matches!(field, ResultField::NodeScalar { .. }) {
                        0
                    } else {
                        2
                    }]);
                    add(&(values.len() as u64).to_le_bytes());
                    add(&min.to_bits().to_le_bytes());
                    add(&max.to_bits().to_le_bytes());
                    for value in values {
                        add(&value.to_bits().to_le_bytes());
                    }
                }
                ResultField::NodeVector {
                    values,
                    min_mag,
                    max_mag,
                    ..
                } => {
                    add(&[1]);
                    add(&(values.len() as u64).to_le_bytes());
                    add(&min_mag.to_bits().to_le_bytes());
                    add(&max_mag.to_bits().to_le_bytes());
                    for value in values {
                        for component in value.to_array() {
                            add(&component.to_bits().to_le_bytes());
                        }
                    }
                }
            }
        }
    }
    hash
}

fn main() -> Result<(), String> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [mesh_path, result_path] = args.as_slice() else {
        return Err("Usage: benchmark_results <matching.msh> <result.res.rank.step>".into());
    };
    let start = Instant::now();
    let mesh = hecmw::load_mesh_file(mesh_path).map_err(|e| e.to_string())?;
    let mesh_ms = start.elapsed().as_secs_f64() * 1000.0;
    let source = std::fs::read_to_string(result_path).map_err(|e| e.to_string())?;
    let source = source.trim_start();
    let parsed = NativeResult::parse(source)?; // validate and warm up
    let read_ms = median_ms(|| std::fs::read_to_string(result_path).map_err(|e| e.to_string()))?;
    let parse_ms = median_ms(|| NativeResult::parse(source))?;
    let map_ms = median_ms(|| parsed.step_for_mesh(&mesh, 0))?;
    let load = || hecmw::result_series::load_mesh_result_series(result_path, &mesh);
    let steps = load()?;
    let series_ms = median_ms(load)?;
    println!(
        "MESH {} nodes={} elements={} first_ms={mesh_ms:.3}",
        mesh_path.display(),
        mesh.nodes.len(),
        mesh.elements.len()
    );
    println!(
        "RES {} bytes={} read_ms={read_ms:.3} parse_ms={parse_ms:.3} map_ms={map_ms:.3}",
        result_path.display(),
        source.len()
    );
    println!(
        "SERIES frames={} fields_first={} repeat_median_ms={series_ms:.3} signature={:016x}",
        steps.len(),
        steps.first().map_or(0, |s| s.fields.len()),
        signature(&mesh, &steps)
    );
    println!("# Five repeats after warmup; destruction excluded; OS cache not cleared.");
    println!(
        "# SERIES includes RES I/O, parsing, ID mapping and unused-node expansion; MSH already loaded."
    );
    println!("# No solver, MPI joining, rendering, GPU upload or UI dialogs.");
    Ok(())
}
