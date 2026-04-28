export type OverlayCandidate = {
  text: string;
  source: string;
};

export type AppState =
  | { type: "Idle"; chord: string }
  | { type: "CollisionRemapRequired"; defaultChord: string }
  | { type: "Recording"; chord: string }
  | { type: "Processing"; chord: string }
  | { type: "ShowingCandidates"; chord: string; candidates: OverlayCandidate[]; lowQuality: boolean; autoInsertThresholdMet: boolean };

export type AppEvent =
  | { type: "CollisionDetected" }
  | { type: "Remapped"; newChord: string }
  | { type: "HotkeyPressed" }
  | { type: "RecordingStopped" }
  | { type: "ProcessingComplete"; candidates: OverlayCandidate[]; lowQuality: boolean; autoInsertThresholdMet: boolean }
  | { type: "NumberKeyPressed"; index: number } // 1-5
  | { type: "MouseSelected"; index: number } // 0-4
  | { type: "EscapePressed" };

export type ActionResult =
  | { type: "None" }
  | { type: "InsertCandidate"; candidate: OverlayCandidate }
  | { type: "Cancel" };

export function reduce(state: AppState, event: AppEvent): { state: AppState; action: ActionResult } {
  switch (state.type) {
    case "Idle":
      if (event.type === "HotkeyPressed") {
        return { state: { type: "Recording", chord: state.chord }, action: { type: "None" } };
      }
      if (event.type === "CollisionDetected") {
        return { state: { type: "CollisionRemapRequired", defaultChord: state.chord }, action: { type: "None" } };
      }
      break;
    case "CollisionRemapRequired":
      if (event.type === "Remapped") {
        if (event.newChord.trim() === "" || event.newChord === state.defaultChord) {
          return { state, action: { type: "None" } };
        }
        return { state: { type: "Idle", chord: event.newChord }, action: { type: "None" } };
      }
      // HotkeyPressed is ignored in this state
      break;
    case "Recording":
      if (event.type === "RecordingStopped") {
        return { state: { type: "Processing", chord: state.chord }, action: { type: "None" } };
      }
      if (event.type === "EscapePressed") {
        return { state: { type: "Idle", chord: state.chord }, action: { type: "Cancel" } };
      }
      break;
    case "Processing":
      if (event.type === "ProcessingComplete") {
        return {
          state: {
            type: "ShowingCandidates",
            chord: state.chord,
            candidates: event.candidates.slice(0, 5),
            lowQuality: event.lowQuality,
            autoInsertThresholdMet: event.autoInsertThresholdMet,
          },
          action: { type: "None" },
        };
      }
      if (event.type === "EscapePressed") {
        return { state: { type: "Idle", chord: state.chord }, action: { type: "Cancel" } };
      }
      break;
    case "ShowingCandidates":
      if (event.type === "NumberKeyPressed") {
        if (event.index >= 1 && event.index <= state.candidates.length) {
          const candidate = state.candidates[event.index - 1];
          return { state: { type: "Idle", chord: state.chord }, action: { type: "InsertCandidate", candidate } };
        }
      }
      if (event.type === "MouseSelected") {
        if (event.index >= 0 && event.index < state.candidates.length) {
          const candidate = state.candidates[event.index];
          return { state: { type: "Idle", chord: state.chord }, action: { type: "InsertCandidate", candidate } };
        }
      }
      if (event.type === "EscapePressed") {
        return { state: { type: "Idle", chord: state.chord }, action: { type: "Cancel" } };
      }
      break;
  }
  return { state, action: { type: "None" } };
}
