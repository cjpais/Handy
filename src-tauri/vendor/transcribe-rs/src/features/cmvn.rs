use ndarray::{Array1, Array2};

/// Apply Cepstral Mean-Variance Normalization (CMVN).
///
/// Formula: `x[i] = (x[i] + neg_mean[i]) * inv_stddev[i]`
///
/// Modifies features in-place.
pub fn apply_cmvn(features: &mut Array2<f32>, neg_mean: &Array1<f32>, inv_stddev: &Array1<f32>) {
    let dim = features.ncols();
    debug_assert_eq!(neg_mean.len(), dim);
    debug_assert_eq!(inv_stddev.len(), dim);

    for mut row in features.rows_mut() {
        for j in 0..dim {
            row[j] = (row[j] + neg_mean[j]) * inv_stddev[j];
        }
    }
}
