use freelip_core::{
    process_roi_frame, FaceLandmarkDetection, FacialLandmarks, FrameQuality, Point, Rect,
    RoiDecisionCode, RoiJitterSmoother, RoiPipelineConfig, RoiPipelineFrame,
};

#[test]
fn roi_pipeline_fixture() {
    let config = RoiPipelineConfig::default();
    let mut smoother = RoiJitterSmoother::new(config.smoothing_alpha);

    let first = process_roi_frame(
        RoiPipelineFrame {
            request_id: "roi-req-0001".to_string(),
            session_id: "session-20260428-0001".to_string(),
            source_kind: "camera".to_string(),
            device_id_hash: Some("sha256:4f2b9a3c".to_string()),
            source_started_at_ms: 1_777_339_200_000,
            requested_at_ms: 1_777_339_203_100,
            frame_count: 75,
            duration_ms: 3_000,
            local_ref: "local://roi/session-20260428-0001/normalized.json".to_string(),
            quality: valid_fixture(0.0),
        },
        &config,
        &mut smoother,
    );

    assert_eq!(first.code, RoiDecisionCode::RoiOk);
    assert!(first.should_emit_sidecar_decode);
    assert_eq!(first.sidecar_decode_requests, 1);
    assert_eq!(first.user_prompt_code, "ROI_OK");
    assert!(first.smoothed_crop_bounds.is_some());

    let request = first
        .roi_request
        .expect("valid ROI should produce request metadata");
    assert_eq!(request.schema_version, "1.0.0");
    assert_eq!(request.request_id, "roi-req-0001");
    assert_eq!(request.session_id, "session-20260428-0001");
    assert_eq!(request.source.kind, "camera");
    assert_eq!(
        request.source.device_id_hash.as_deref(),
        Some("sha256:4f2b9a3c")
    );
    assert_eq!(
        request.roi.local_ref,
        "local://roi/session-20260428-0001/normalized.json"
    );
    assert_eq!(request.roi.format, "grayscale_u8");
    assert_eq!(request.roi.width, 96);
    assert_eq!(request.roi.height, 96);
    assert_eq!(request.roi.cnvsrc_center_crop_size, 88);
    assert_eq!(
        request.roi.cnvsrc_compatibility_note,
        "96x96 grayscale_u8; center-crop to 88x88 before CNVSRC normalization mean=0.421 std=0.165"
    );
    assert_eq!(request.roi.fps, 25.0);
    assert_eq!(request.roi.frame_count, 75);
    assert_eq!(request.roi.duration_ms, 3_000);
    assert!(request.quality_flags.face_found);
    assert!(request.quality_flags.mouth_landmarks_found);
    assert!(request.quality_flags.crop_bounds_valid);
    assert!(request.quality_flags.blur_ok);
    assert!(request.quality_flags.brightness_ok);
    assert!(request.quality_flags.occlusion_ok);
    assert!(request.quality_flags.rejection_reasons.is_empty());

    let first_crop = first.smoothed_crop_bounds.expect("first crop should exist");
    let second = process_roi_frame(
        RoiPipelineFrame {
            request_id: "roi-req-0002".to_string(),
            session_id: "session-20260428-0001".to_string(),
            source_kind: "camera".to_string(),
            device_id_hash: Some("sha256:4f2b9a3c".to_string()),
            source_started_at_ms: 1_777_339_200_000,
            requested_at_ms: 1_777_339_203_140,
            frame_count: 76,
            duration_ms: 3_040,
            local_ref: "local://roi/session-20260428-0001/normalized-2.json".to_string(),
            quality: valid_fixture(24.0),
        },
        &config,
        &mut smoother,
    );
    let second_raw = second
        .raw_crop_bounds
        .expect("valid shifted frame should have raw crop");
    let second_smoothed = second
        .smoothed_crop_bounds
        .expect("valid shifted frame should have smoothed crop");

    assert!(second_smoothed.x > first_crop.x);
    assert!(second_smoothed.x < second_raw.x);
    assert_eq!(second.frame_summary.accepted_frames, 1);
    assert_eq!(second.frame_summary.rejected_frames, 0);
    println!(
        "ROI_PIPELINE_OK local_ref={} crop={}x{}@{:.1},{:.1} smoothed_x={:.1} raw_x={:.1}",
        request.roi.local_ref,
        request.roi.width,
        request.roi.height,
        first_crop.x,
        first_crop.y,
        second_smoothed.x,
        second_raw.x
    );
}

#[test]
fn roi_rejects_bad_inputs() {
    let config = RoiPipelineConfig::default();
    let cases = [
        ("NO_FACE", no_face_fixture()),
        ("MOUTH_OCCLUDED", occluded_mouth_fixture()),
        ("LOW_LIGHT", low_light_fixture()),
        ("BLURRY", blurry_fixture()),
        ("CROP_OUT_OF_BOUNDS", crop_out_of_bounds_fixture()),
    ];

    for (expected_code, quality) in cases {
        let mut smoother = RoiJitterSmoother::new(config.smoothing_alpha);
        let decision = process_roi_frame(
            RoiPipelineFrame {
                request_id: format!("roi-reject-{expected_code}"),
                session_id: "session-20260428-0002".to_string(),
                source_kind: "fixture".to_string(),
                device_id_hash: None,
                source_started_at_ms: 1_777_339_200_000,
                requested_at_ms: 1_777_339_203_100,
                frame_count: 1,
                duration_ms: 40,
                local_ref: "local://roi/session-20260428-0002/rejected.json".to_string(),
                quality,
            },
            &config,
            &mut smoother,
        );

        assert_eq!(decision.user_prompt_code, expected_code);
        assert!(!decision.should_emit_sidecar_decode);
        assert_eq!(decision.sidecar_decode_requests, 0);
        assert!(decision.roi_request.is_none());
        assert_eq!(decision.frame_summary.accepted_frames, 0);
        assert_eq!(decision.frame_summary.rejected_frames, 1);
        assert!(!decision.quality_flags.rejection_reasons.is_empty());
        println!(
            "ROI_REJECT {expected_code} sidecar_decode_requests={} reasons={:?}",
            decision.sidecar_decode_requests, decision.quality_flags.rejection_reasons
        );
    }
}

fn valid_fixture(x_shift: f32) -> FrameQuality {
    FrameQuality {
        frame_width: 640,
        frame_height: 480,
        brightness: 0.62,
        blur_score: 0.74,
        face: Some(FaceLandmarkDetection {
            face_bounds: Rect {
                x: 210.0 + x_shift,
                y: 80.0,
                width: 220.0,
                height: 260.0,
            },
            landmarks: FacialLandmarks {
                right_eye: Some(Point {
                    x: 270.0 + x_shift,
                    y: 165.0,
                }),
                left_eye: Some(Point {
                    x: 370.0 + x_shift,
                    y: 166.0,
                }),
                nose_tip: Some(Point {
                    x: 322.0 + x_shift,
                    y: 222.0,
                }),
                right_mouth_corner: Some(Point {
                    x: 285.0 + x_shift,
                    y: 282.0,
                }),
                left_mouth_corner: Some(Point {
                    x: 360.0 + x_shift,
                    y: 283.0,
                }),
            },
            confidence: 0.93,
        }),
    }
}

fn no_face_fixture() -> FrameQuality {
    FrameQuality {
        face: None,
        ..valid_fixture(0.0)
    }
}

fn occluded_mouth_fixture() -> FrameQuality {
    let mut frame = valid_fixture(0.0);
    let face = frame
        .face
        .as_mut()
        .expect("valid fixture should include face");
    face.landmarks.right_mouth_corner = None;
    face.landmarks.left_mouth_corner = None;
    frame
}

fn low_light_fixture() -> FrameQuality {
    FrameQuality {
        brightness: 0.10,
        ..valid_fixture(0.0)
    }
}

fn blurry_fixture() -> FrameQuality {
    FrameQuality {
        blur_score: 0.10,
        ..valid_fixture(0.0)
    }
}

fn crop_out_of_bounds_fixture() -> FrameQuality {
    let mut frame = valid_fixture(0.0);
    let face = frame
        .face
        .as_mut()
        .expect("valid fixture should include face");
    face.landmarks.right_mouth_corner = Some(Point { x: 5.0, y: 282.0 });
    face.landmarks.left_mouth_corner = Some(Point { x: 80.0, y: 283.0 });
    frame
}
