use super::*;

#[test]
fn rows_are_laid_out_from_the_top_of_the_viewport() {
    let m = Metrics::new(800.0, 400.0, 1.0);
    let first = m.row_rect(0, 0.0);
    assert_eq!(first.y, m.rows_top());
    assert_eq!(first.h, ROW_H);
    assert_eq!(first.w, 800.0 - PAD * 2.0);

    let second = m.row_rect(1, 10.0);
    assert_eq!(second.y, m.rows_top() + ROW_H - 10.0);
}

#[test]
fn scrolling_clamps_to_the_rows_that_exist() {
    let m = Metrics::new(800.0, 400.0, 1.0);
    assert_eq!(m.max_scroll(0), 0.0);
    let rows_that_fit = (m.rows_viewport_h() / m.pitch()).floor() as usize;
    assert_eq!(m.max_scroll(rows_that_fit), 0.0);
    let extra = (rows_that_fit + 3) as f32 * m.pitch() - m.rows_viewport_h();
    assert_eq!(m.max_scroll(rows_that_fit + 3), extra);
}

#[test]
fn a_point_maps_to_the_row_under_it() {
    let m = Metrics::new(800.0, 400.0, 1.0);
    assert_eq!(m.row_at(m.rows_top(), 0.0, 3), Some(0));
    assert_eq!(m.row_at(m.rows_top() + m.pitch(), 0.0, 3), Some(1));
    // Above the header and past the last row there is nothing.
    assert_eq!(m.row_at(0.0, 0.0, 3), None);
    assert_eq!(m.row_at(m.rows_top() + 3.0 * m.pitch(), 0.0, 3), None);
}

#[test]
fn whole_rows_are_derived_from_the_window_height() {
    for rows in 1..20 {
        let height = height_for_rows(rows, 1.0);
        assert_eq!(rows_in_height(height as f32, 1.0), rows);
    }
    assert_eq!(rows_in_height(0.0, 1.0), 1);
}

#[test]
fn every_length_scales_with_the_monitor() {
    let m = Metrics::new(1600.0, 800.0, 2.0);
    assert_eq!(m.row_h, ROW_H * 2.0);
    assert_eq!(m.header_h, HEADER_H * 2.0);
    assert_eq!(device_px(100, 1.5), 150);
    assert_eq!(logical_px(150, 1.5), 100);
    // Round-trips never collapse to zero.
    assert_eq!(logical_px(1, 4.0), 1);
}

#[test]
fn physical_density_comes_from_the_reported_glass_size() {
    // A 24-inch 1920x1080 panel is about 92 PPI, i.e. slightly under 96.
    let scale = physical_scale(1920, 1080, 531.0, 299.0).expect("plausible monitor");
    assert!((scale - 0.96).abs() < 0.05);

    // 27-inch 4K is about 163 PPI.
    let scale = physical_scale(3840, 2160, 597.0, 336.0).expect("plausible monitor");
    assert!((scale - 1.7).abs() < 0.1);

    // Missing or nonsense measurements fall back to the Windows DPI guess.
    assert!(physical_scale(0, 1080, 531.0, 299.0).is_none());
    assert!(physical_scale(1920, 1080, 0.0, 0.0).is_none());
    assert!(physical_scale(1920, 1080, 10_000.0, 10_000.0).is_none());
}
