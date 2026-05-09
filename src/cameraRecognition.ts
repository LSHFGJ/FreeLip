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

export type RunLiveRoiDecodeWhenReadyOptions = {
  cameraReady: boolean;
  modelReady: boolean;
  prepareRoiRequest: () => Promise<RoiRequest>;
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
      message: "Camera preview is active, but MediaRecorder/canvas ROI capture is unavailable in this WebView."
    };
  }

  return {
    code: "READY_TO_DECODE",
    realRecognition: true,
    message: "Camera clip capture, model readiness, and local ROI transport are ready for real /decode. Mouth-landmark crop quality remains conservative."
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
}: RunDecodeWhenReadyOptions): Promise<boolean> {
  if (!cameraReady || !modelReady || !roiRequest) {
    return false;
  }

  const result = await decode(roiRequest);
  if (!result.realDecode) {
    return false;
  }

  dispatch({
    type: "ProcessingComplete",
    candidates: mapDecodeCandidatesToOverlay(result.candidates),
    lowQuality: result.response.quality_flags.rejection_reasons.length > 0,
    autoInsertThresholdMet: result.candidates[0]?.is_auto_insert_eligible === true
  });
  return true;
}

export async function runLiveRoiDecodeWhenReady({
  cameraReady,
  modelReady,
  prepareRoiRequest,
  decode,
  dispatch
}: RunLiveRoiDecodeWhenReadyOptions): Promise<boolean> {
  if (!cameraReady || !modelReady) {
    return false;
  }

  const roiRequest = await prepareRoiRequest();
  return runDecodeWhenReady({
    cameraReady,
    modelReady,
    roiRequest,
    decode,
    dispatch
  });
}
