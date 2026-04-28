# ROI landmark model selection

FreeLip selects OpenCV Zoo YuNet `face_detection_yunet_2023mar.onnx` as the Task 3 Windows-local ONNX face/landmark model.

## Source and license

- Upstream repository: <https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet>
- Model file: <https://github.com/opencv/opencv_zoo/blob/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx>
- Upstream training/model source noted by OpenCV Zoo: <https://github.com/ShiqiYu/libfacedetection.train/blob/a61a428929148171b488f024b5d6774f93cdbc13/tasks/task1/onnx/yunet.onnx>
- License: MIT License for all files in `models/face_detection_yunet` per OpenCV Zoo.
- Git LFS SHA-256: `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4`
- File size: `232589` bytes

## Why this model

- It is small enough for local Windows desktop ROI bootstrap and CPU fallback experimentation.
- OpenCV Zoo documents YuNet as a lightweight face detector and ships it as ONNX.
- The OpenCV demo layout returns a face box, five landmarks, and confidence. The five landmarks are right eye, left eye, nose tip, right mouth corner, and left mouth corner, which is enough for Task 7 to compute an initial mouth ROI crop and quality gates.

## Runtime handling in this repo

The model binary is not committed. `.gitignore` keeps `.onnx`, model weights, datasets, and media ignored. Developers who need the real model should download it locally into the ignored `models/` directory and verify the SHA-256 above before use.

Task 3 tests generate a tiny ONNX Identity fixture at test runtime and load it with `tract-onnx`. That proves the Rust ONNX model-loading path without committing model binaries or depending on cloud APIs.

## Landmark layout for ROI consumers

FreeLip treats YuNet detections as:

1. Bounding box: `x`, `y`, `width`, `height`
2. Right eye point
3. Left eye point
4. Nose tip point
5. Right mouth corner point
6. Left mouth corner point
7. Detection confidence

The Task 3 ROI quality API exposes reason codes for `NO_FACE`, `MOUTH_OCCLUDED`, `LOW_LIGHT`, `BLURRY`, `CROP_OUT_OF_BOUNDS`, and `ROI_OK` so Task 7 can reject bad frames before any VSR sidecar request is created.
