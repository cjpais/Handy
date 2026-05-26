use ndarray::Array2;

/// Apply Lower Frame Rate (LFR) stacking.
///
/// Concatenates `window_size` consecutive frames with a stride of `window_shift`,
/// reducing temporal resolution while increasing feature dimension.
///
/// Input shape: [num_frames, feat_dim]
/// Output shape: [(num_frames - window_size) / window_shift + 1, feat_dim * window_size]
pub fn apply_lfr(features: &Array2<f32>, window_size: usize, window_shift: usize) -> Array2<f32> {
    let in_frames = features.nrows();
    let in_dim = features.ncols();

    if in_frames < window_size {
        return Array2::zeros((0, in_dim * window_size));
    }

    let out_frames = (in_frames - window_size) / window_shift + 1;
    let out_dim = in_dim * window_size;

    let mut out = Array2::zeros((out_frames, out_dim));

    for i in 0..out_frames {
        let src_start = i * window_shift;
        for w in 0..window_size {
            let src_row = features.row(src_start + w);
            let dst_start = w * in_dim;
            for (j, &val) in src_row.iter().enumerate() {
                out[[i, dst_start + j]] = val;
            }
        }
    }

    out
}
