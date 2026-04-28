use freelip_core::{
    evaluate_roi_quality, FaceLandmarkDetection, FacialLandmarks, FrameQuality, Point, Rect,
    RoiDecisionCode, RoiQualityThresholds,
};

#[test]
fn roi_quality_fixture() {
    let thresholds = RoiQualityThresholds::default();

    assert_eq!(
        evaluate_roi_quality(&valid_fixture(), &thresholds).code,
        RoiDecisionCode::RoiOk
    );
    assert_eq!(
        evaluate_roi_quality(&no_face_fixture(), &thresholds).code,
        RoiDecisionCode::NoFace
    );
    assert_eq!(
        evaluate_roi_quality(&occluded_mouth_fixture(), &thresholds).code,
        RoiDecisionCode::MouthOccluded
    );
}

#[test]
fn roi_quality_valid_fixture() {
    let report = evaluate_roi_quality(&valid_fixture(), &RoiQualityThresholds::default());

    assert_eq!(report.code, RoiDecisionCode::RoiOk);
    assert!(report.face_found);
    assert!(report.mouth_landmarks_found);
    assert!(report.crop_bounds_valid);
    assert!(
        report.quality_score >= 0.80,
        "quality score was {}",
        report.quality_score
    );

    let crop = report
        .crop_bounds
        .expect("valid fixture should return crop");
    println!(
        "ROI_OK crop={}x{}@{},{} confidence={:.2} quality={:.2}",
        crop.width, crop.height, crop.x, crop.y, report.confidence, report.quality_score
    );
}

#[test]
fn roi_quality_reject_no_face() {
    let report = evaluate_roi_quality(&no_face_fixture(), &RoiQualityThresholds::default());

    assert_eq!(report.code, RoiDecisionCode::NoFace);
    assert!(!report.face_found);
    assert!(report.crop_bounds.is_none());
    println!("ROI_REJECT {}", report.code.as_str());
}

#[test]
fn roi_quality_reject_occluded_mouth() {
    let report = evaluate_roi_quality(&occluded_mouth_fixture(), &RoiQualityThresholds::default());

    assert_eq!(report.code, RoiDecisionCode::MouthOccluded);
    assert!(report.face_found);
    assert!(!report.mouth_landmarks_found);
    assert!(report.crop_bounds.is_none());
    println!("ROI_REJECT {}", report.code.as_str());
}

fn valid_fixture() -> FrameQuality {
    FrameQuality {
        frame_width: 640,
        frame_height: 480,
        brightness: 0.62,
        blur_score: 0.74,
        face: Some(FaceLandmarkDetection {
            face_bounds: Rect {
                x: 210.0,
                y: 80.0,
                width: 220.0,
                height: 260.0,
            },
            landmarks: FacialLandmarks {
                right_eye: Some(Point { x: 270.0, y: 165.0 }),
                left_eye: Some(Point { x: 370.0, y: 166.0 }),
                nose_tip: Some(Point { x: 322.0, y: 222.0 }),
                right_mouth_corner: Some(Point { x: 285.0, y: 282.0 }),
                left_mouth_corner: Some(Point { x: 360.0, y: 283.0 }),
            },
            confidence: 0.93,
        }),
    }
}

fn no_face_fixture() -> FrameQuality {
    FrameQuality {
        frame_width: 640,
        frame_height: 480,
        brightness: 0.61,
        blur_score: 0.70,
        face: None,
    }
}

fn occluded_mouth_fixture() -> FrameQuality {
    let mut frame = valid_fixture();
    let face = frame
        .face
        .as_mut()
        .expect("valid fixture should include face");
    face.landmarks.right_mouth_corner = None;
    face.landmarks.left_mouth_corner = None;
    frame
}
