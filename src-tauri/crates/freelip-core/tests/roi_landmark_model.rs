use prost::Message;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tract_onnx::pb::tensor_proto::DataType;
use tract_onnx::pb::tensor_shape_proto::dimension::Value as DimensionValue;
use tract_onnx::pb::type_proto::Value as TypeValue;
use tract_onnx::pb::{
    attribute_proto::AttributeType, AttributeProto, GraphProto, ModelProto, NodeProto,
    OperatorSetIdProto, TensorProto, TensorShapeProto, TypeProto, ValueInfoProto,
};

use freelip_core::{
    face_detection_from_yunet_row, load_roi_landmark_model, run_no_input_onnx_f32_output,
    SELECTED_ROI_MODEL,
};

#[test]
fn roi_landmark_model_loads() {
    let model_path = fixture_path("freelip-roi-identity", "onnx");
    write_identity_onnx(&model_path);

    let summary = load_roi_landmark_model(&model_path).expect("tiny ONNX fixture should load");

    assert_eq!(summary.runtime, "tract-onnx");
    assert_eq!(summary.input_count, 1);
    assert_eq!(summary.output_count, 1);
    assert_eq!(SELECTED_ROI_MODEL.model_id, "opencv-yunet-2023mar");
    assert_eq!(
        SELECTED_ROI_MODEL.file_name,
        "face_detection_yunet_2023mar.onnx"
    );
    assert_eq!(SELECTED_ROI_MODEL.license, "MIT");
    assert_eq!(SELECTED_ROI_MODEL.size_bytes, 232_589);
    assert_eq!(
        SELECTED_ROI_MODEL.sha256,
        "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4"
    );

    println!(
        "ROI_MODEL {} runtime={} fixture={} license={}",
        SELECTED_ROI_MODEL.model_id,
        summary.runtime,
        summary.graph_name,
        SELECTED_ROI_MODEL.license
    );

    let _ = fs::remove_file(model_path);
}

#[test]
fn roi_landmark_fixture_outputs_mouth_landmarks() {
    let model_path = fixture_path("freelip-roi-yunet-output", "onnx");
    write_yunet_constant_onnx(&model_path);

    let output = run_no_input_onnx_f32_output(&model_path)
        .expect("tiny YuNet-shaped ONNX fixture should run");
    let detection = face_detection_from_yunet_row(&output)
        .expect("YuNet-shaped output should map to landmarks");

    assert_eq!(detection.face_bounds.x, 210.0);
    assert_eq!(detection.face_bounds.y, 80.0);
    assert_eq!(
        detection.landmarks.right_mouth_corner,
        Some(freelip_core::Point { x: 285.0, y: 282.0 })
    );
    assert_eq!(
        detection.landmarks.left_mouth_corner,
        Some(freelip_core::Point { x: 360.0, y: 283.0 })
    );
    assert_eq!(detection.confidence, 0.93);

    println!(
        "ROI_LANDMARKS mouth_right=285,282 mouth_left=360,283 confidence={:.2}",
        detection.confidence
    );

    let _ = fs::remove_file(model_path);
}

fn fixture_path(prefix: &str, extension: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos}.{extension}",
        std::process::id()
    ))
}

fn write_identity_onnx(path: &Path) {
    let model = ModelProto {
        ir_version: 7,
        opset_import: vec![OperatorSetIdProto {
            domain: String::new(),
            version: 13,
        }],
        producer_name: "freelip-core-test".to_string(),
        producer_version: "0.1.0".to_string(),
        domain: "local.freelip.test".to_string(),
        model_version: 1,
        doc_string: "Tiny Identity graph generated at test runtime.".to_string(),
        graph: Some(GraphProto {
            node: vec![NodeProto {
                input: vec!["input".to_string()],
                output: vec!["output".to_string()],
                name: "identity_node".to_string(),
                op_type: "Identity".to_string(),
                domain: String::new(),
                attribute: vec![],
                doc_string: String::new(),
            }],
            name: "freelip_identity_fixture".to_string(),
            initializer: vec![],
            sparse_initializer: vec![],
            doc_string: String::new(),
            input: vec![value_info("input")],
            output: vec![value_info("output")],
            value_info: vec![],
            quantization_annotation: vec![],
        }),
        metadata_props: vec![],
        training_info: vec![],
        functions: vec![],
    };

    let mut bytes = Vec::new();
    model
        .encode(&mut bytes)
        .expect("fixture ONNX should encode");
    fs::write(path, bytes).expect("fixture ONNX should be written");
}

fn write_yunet_constant_onnx(path: &Path) {
    let detection_row = vec![
        210.0, 80.0, 220.0, 260.0, 270.0, 165.0, 370.0, 166.0, 322.0, 222.0, 285.0, 282.0, 360.0,
        283.0, 0.93,
    ];
    let model = ModelProto {
        ir_version: 7,
        opset_import: vec![OperatorSetIdProto {
            domain: String::new(),
            version: 13,
        }],
        producer_name: "freelip-core-test".to_string(),
        producer_version: "0.1.0".to_string(),
        domain: "local.freelip.test".to_string(),
        model_version: 1,
        doc_string: "Tiny Constant graph generated at test runtime.".to_string(),
        graph: Some(GraphProto {
            node: vec![NodeProto {
                input: vec![],
                output: vec!["detections".to_string()],
                name: "yunet_constant_detection".to_string(),
                op_type: "Constant".to_string(),
                domain: String::new(),
                attribute: vec![AttributeProto {
                    name: "value".to_string(),
                    ref_attr_name: String::new(),
                    doc_string: String::new(),
                    r#type: AttributeType::Tensor as i32,
                    f: 0.0,
                    i: 0,
                    s: vec![],
                    t: Some(TensorProto {
                        dims: vec![1, 15],
                        data_type: DataType::Float as i32,
                        segment: None,
                        float_data: detection_row,
                        int32_data: vec![],
                        string_data: vec![],
                        int64_data: vec![],
                        name: "fixture_yunet_detection".to_string(),
                        doc_string: String::new(),
                        raw_data: vec![],
                        double_data: vec![],
                        uint64_data: vec![],
                        data_location: None,
                        external_data: vec![],
                    }),
                    g: None,
                    sparse_tensor: None,
                    floats: vec![],
                    ints: vec![],
                    strings: vec![],
                    tensors: vec![],
                    graphs: vec![],
                    sparse_tensors: vec![],
                    type_protos: vec![],
                }],
                doc_string: String::new(),
            }],
            name: "freelip_yunet_output_fixture".to_string(),
            initializer: vec![],
            sparse_initializer: vec![],
            doc_string: String::new(),
            input: vec![],
            output: vec![value_info_with_shape("detections", &[1, 15])],
            value_info: vec![],
            quantization_annotation: vec![],
        }),
        metadata_props: vec![],
        training_info: vec![],
        functions: vec![],
    };

    let mut bytes = Vec::new();
    model
        .encode(&mut bytes)
        .expect("YuNet fixture ONNX should encode");
    fs::write(path, bytes).expect("YuNet fixture ONNX should be written");
}

fn value_info(name: &str) -> ValueInfoProto {
    value_info_with_shape(name, &[1, 1])
}

fn value_info_with_shape(name: &str, shape: &[i64]) -> ValueInfoProto {
    ValueInfoProto {
        name: name.to_string(),
        r#type: Some(TypeProto {
            denotation: String::new(),
            value: Some(TypeValue::TensorType(tract_onnx::pb::type_proto::Tensor {
                elem_type: DataType::Float as i32,
                shape: Some(TensorShapeProto {
                    dim: shape.iter().copied().map(dimension).collect(),
                }),
            })),
        }),
        doc_string: String::new(),
    }
}

fn dimension(value: i64) -> tract_onnx::pb::tensor_shape_proto::Dimension {
    tract_onnx::pb::tensor_shape_proto::Dimension {
        denotation: String::new(),
        value: Some(DimensionValue::DimValue(value)),
    }
}
