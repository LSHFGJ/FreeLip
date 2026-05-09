import type { AppEvent, OverlayCandidate } from "./hotkeyState.ts";
import type { CandidateResponseCandidate, DecodeResult, RoiRequest } from "./sidecarDecode.ts";

export type CameraRecognitionStatus = {
  code: "CAMERA_NOT_READY" | "MODEL_NOT_READY" | "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED" | "READY_TO_DECODE";
  realRecognition: boolean;
  message: string;
};

export type RunDecodeWhenReadyOptions = {
  cameraReady: boolean;
  modelReady: boolean;
  roiRequest: RoiRequest | null;
  decode: (request: RoiRequest) => Promise<DecodeResult>;
  dispatch: (event: AppEvent) => void;
};

export function createCameraRecognitionStatus(input: {
  cameraReady: boolean;
  modelReady: boolean;
  roiReady: boolean;
}): CameraRecognitionStatus {
  if (!input.cameraReady) {
    return {
      code: "CAMERA_NOT_READY",
      realRecognition: false,
      message: "Camera preview has not started yet."
    };
  }

  if (!input.modelReady) {
    return {
      code: "MODEL_NOT_READY",
      realRecognition: false,
      message: "CNVSRC runtime is not ready yet."
    };
  }

  if (!input.roiReady) {
    return {
      code: "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED",
      realRecognition: false,
      message: "Camera preview is active, but live ROI capture is not wired to /decode yet."
    };
  }

  return {
    code: "READY_TO_DECODE",
    realRecognition: true,
    message: "Camera, model, and ROI transport are ready for real /decode recognition."
  };
}

export function mapDecodeCandidatesToOverlay(candidates: CandidateResponseCandidate[]): OverlayCandidate[] {
  return candidates.slice(0, 5).map((candidate) => ({
    text: candidate.text,
    source: candidate.source
  }));
}

export async function runDecodeWhenReady({
  cameraReady,
  modelReady,
  roiRequest,
  decode,
  dispatch
}: RunDecodeWhenReadyOptions): Promise<void> {
  if (!cameraReady || !modelReady || !roiRequest) {
    return;
  }

  const result = await decode(roiRequest);
  if (!result.realDecode) {
    return;
  }

  dispatch({
    type: "ProcessingComplete",
    candidates: mapDecodeCandidatesToOverlay(result.candidates),
    lowQuality: result.response.quality_flags.rejection_reasons.length > 0,
    autoInsertThresholdMet: result.candidates[0]?.is_auto_insert_eligible === true
  });
}
