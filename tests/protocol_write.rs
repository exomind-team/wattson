use wattson::protocol::{
    build_fan_curve_frame, build_fan_mode_frame, build_query_frame, crc8, expand_fan_curve_table,
    FanMode,
};

#[test]
fn build_query_frame_matches_wire_bytes() {
    assert_eq!(
        build_query_frame(),
        vec![0x55, 0x7E, 0x02, 0x04, 0x06, 0xAE]
    );
}

#[test]
fn build_fan_mode_frames_match_protocol_table() {
    assert_eq!(
        build_fan_mode_frame(FanMode::Auto),
        vec![0x55, 0x7E, 0x04, 0x13, 0x00, 0x00, 0x17, 0xAE]
    );
    assert_eq!(
        build_fan_mode_frame(FanMode::Silent),
        vec![0x55, 0x7E, 0x04, 0x13, 0x00, 0x01, 0x18, 0xAE]
    );
    assert_eq!(
        build_fan_mode_frame(FanMode::Performance),
        vec![0x55, 0x7E, 0x04, 0x13, 0x00, 0x02, 0x19, 0xAE]
    );
    assert_eq!(
        build_fan_mode_frame(FanMode::Custom),
        vec![0x55, 0x7E, 0x04, 0x13, 0x00, 0x03, 0x20, 0xAE]
    );
    assert_eq!(
        build_fan_mode_frame(FanMode::Clean),
        vec![0x55, 0x7E, 0x04, 0x13, 0x00, 0x04, 0x21, 0xAE]
    );
}

#[test]
fn crc8_matches_flat_curve_probe_sample() {
    let mut payload = vec![0x1D, 0x1B];
    payload.extend(std::iter::repeat_n(0x50, 21));
    payload.extend([0x1E, 0x50, 0x3C, 0x50, 0x5A, 0x50]);

    assert_eq!(crc8(&payload), 0x63);
}

#[test]
fn expand_fan_curve_table_accepts_three_control_points() {
    let samples = expand_fan_curve_table(&[(40, 20), (60, 30), (80, 70)]).expect("curve table");

    assert_eq!(samples.len(), 21);
    assert_eq!(samples.first().copied(), Some(0));
    assert_eq!(samples[8], 20);
    assert_eq!(samples[12], 30);
    assert_eq!(samples[16], 70);
    assert_eq!(samples.last().copied(), Some(100));
}

#[test]
fn build_fan_curve_frame_matches_flat_pwm_probe_sample() {
    let frame = build_fan_curve_frame(&[(0, 80), (30, 80), (60, 80), (90, 80), (100, 80)])
        .expect("curve frame");

    let mut expected = vec![0x55, 0x7E, 0x1D, 0x1B];
    expected.extend(std::iter::repeat_n(0x50, 21));
    expected.extend([0x1E, 0x50, 0x3C, 0x50, 0x5A, 0x50, 0x63, 0xAE]);

    assert_eq!(frame, expected);
}
