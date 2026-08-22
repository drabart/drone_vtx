/// Downsamples packed YUYV (Y0 U0 Y1 V0) to planar YUV420P
pub fn yuyv_to_yuv420p(
    yuyv: &[u8],
    y_plane: &mut [u8],
    u_plane: &mut [u8],
    v_plane: &mut [u8],
    width: usize,
    height: usize,
) {
    let mut y_idx = 0;
    let mut uv_idx = 0;

    for row in 0..height {
        let row_offset = row * width * 2;
        let is_even_row = row % 2 == 0;

        for col in (0..(width * 2)).step_by(4) {
            let idx = row_offset + col;

            // Extract Y values (Every 1st and 3rd byte in YUYV)
            y_plane[y_idx] = yuyv[idx];
            y_plane[y_idx + 1] = yuyv[idx + 2];
            y_idx += 2;

            // Subsample U and V (Take U/V from every second row/column pair)
            if is_even_row {
                u_plane[uv_idx] = yuyv[idx + 1];
                v_plane[uv_idx] = yuyv[idx + 3];
                uv_idx += 1;
            }
        }
    }
}
