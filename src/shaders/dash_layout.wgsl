// How a run of a wire lays a dash pattern out, at the current linetype scale.
// `run_total` says which layout applies: above zero the run is "A"-aligned and
// is that long, below zero it is a closed path of that length, and at zero the
// pattern tiles from the run's start. Mirrors `wire_gpu.rs`.

// The length of the dash at each end of an "A"-aligned run:
//   [D] [gap] ([dash] [gap])*(k-1) [D]  =>  D = (L - k*P + a) / 2
// A run no longer than one period is one solid dash.
fn aligned_end_dash(run_total: f32, pat_len: f32, first: f32) -> f32 {
    if run_total <= 0.0 {
        return 0.0;
    }
    if run_total <= pat_len {
        return run_total;
    }
    var periods = max(floor(run_total / pat_len), 1.0);
    var end_dash = (run_total - periods * pat_len + first) * 0.5;
    if end_dash <= 1e-4 {
        periods = max(periods - 1.0, 0.0);
        end_dash = (run_total - periods * pat_len + first) * 0.5;
    }
    return clamp(end_dash, 1e-4, run_total * 0.5);
}

// A closed path shorter than a period is drawn solid. Its length is that of its
// chords, a little under a curve's own: one period exactly still shows dashes.
fn closed_dash_solid(length: f32, pat_len: f32) -> bool {
    return length < pat_len * 0.999;
}

// The position in the pattern of a closed path, which holds a whole number of
// periods, each stretched to fit: the nearest count, the lower on a tie, and at
// least two.
fn closed_dash_phase(dist: f32, length: f32, pat_len: f32) -> f32 {
    let periods = max(ceil(length / pat_len - 0.5001), 2.0);
    let stretched = dist * periods * pat_len / length;
    return ((stretched % pat_len) + pat_len) % pat_len;
}

// A thin wire's segment carries its run's layout in the two 16-bit slots a
// wide band uses for its widths: the bits of `run_total`, low half first.
fn dash_run_total(low: i32, high: i32) -> f32 {
    return bitcast<f32>((u32(low) & 0xffffu) | (u32(high) << 16u));
}
