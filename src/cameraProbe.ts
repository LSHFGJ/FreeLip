type CameraMediaDevices = Pick<MediaDevices, "getUserMedia">;

type CameraProbeButton = {
  disabled: boolean;
  addEventListener: (event: "click", listener: () => void | Promise<void>) => void;
};

type CameraProbeStatus = {
  textContent: string | null;
  className: string;
};

type CameraProbeVideo = {
  srcObject: MediaProvider | null;
  hidden: boolean;
  muted: boolean;
  playsInline: boolean;
};

export type CameraProbeControllerOptions = {
  button: CameraProbeButton;
  status: CameraProbeStatus;
  video: CameraProbeVideo;
  getMediaDevices: () => CameraMediaDevices | null | undefined;
};

const cameraConstraints: MediaStreamConstraints = { video: true, audio: false };

function setStatus(status: CameraProbeStatus, state: "idle" | "pending" | "ready" | "error", text: string) {
  status.textContent = text;
  status.className = `camera-status camera-status-${state}`;
}

export function createCameraProbeController({
  button,
  status,
  video,
  getMediaDevices
}: CameraProbeControllerOptions) {
  button.addEventListener("click", async () => {
    button.disabled = true;
    setStatus(status, "pending", "Requesting camera permission for a local preview...");

    const mediaDevices = getMediaDevices();
    if (!mediaDevices?.getUserMedia) {
      setStatus(
        status,
        "error",
        "Camera access is unavailable in this WebView/browser context. Rebuild and run the Windows debug app to test the real permission prompt."
      );
      button.disabled = false;
      return;
    }

    try {
      const stream = await mediaDevices.getUserMedia(cameraConstraints);
      video.srcObject = stream;
      video.hidden = false;
      video.muted = true;
      video.playsInline = true;
      setStatus(
        status,
        "ready",
        "Camera preview is active. This only verifies camera permission and local capture, not ROI cropping or model inference."
      );
    } catch (error) {
      const reason = error instanceof Error && error.message ? ` ${error.message}` : "";
      setStatus(status, "error", `Camera permission or capture failed.${reason}`);
    } finally {
      button.disabled = false;
    }
  });
}
