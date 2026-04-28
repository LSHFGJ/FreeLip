import { reduce } from "../src/hotkeyState.ts";
import assert from "node:assert";

class MockElement {
  children: any[] = [];
  className = "";
  textContent = "";
  innerHTML = "";
  attributes: Record<string, string> = {};
  tagName: string;

  constructor(tagName: string) {
    this.tagName = tagName;
  }

  setAttribute(name: string, value: string) {
    this.attributes[name] = value;
  }
  
  appendChild(child: any) {
    this.children.push(child);
  }
}

const mockApp = new MockElement("div");
(globalThis as any).document = {
  querySelector: (sel: string) => {
    if (sel === "#app") return mockApp;
    return null;
  },
  querySelectorAll: () => [],
  createElement: (tag: string) => new MockElement(tag),
  createTextNode: (text: string) => ({ textContent: text, isTextNode: true }),
  addEventListener: () => {},
  body: new MockElement("body"),
};

import { renderCandidates } from "../src/render.ts";

function runTests() {
  // Test Idle
  let result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "HotkeyPressed" });
  assert.deepStrictEqual(result.state, { type: "Recording", chord: "Ctrl+Alt+Space" });

  // Test Collision
  result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "CollisionDetected" });
  assert.deepStrictEqual(result.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });
  
  // Test remap ignores HotkeyPressed
  result = reduce(result.state, { type: "HotkeyPressed" });
  assert.deepStrictEqual(result.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });
  
  // Test remap ignores empty or same chord
  let remapResult = reduce({ type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" }, { type: "Remapped", newChord: "   " });
  assert.deepStrictEqual(remapResult.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });

  remapResult = reduce({ type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" }, { type: "Remapped", newChord: "Ctrl+Alt+Space" });
  assert.deepStrictEqual(remapResult.state, { type: "CollisionRemapRequired", defaultChord: "Ctrl+Alt+Space" });

  result = reduce(result.state, { type: "Remapped", newChord: "Ctrl+Alt+L" });
  assert.deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+L" });
  
  // Now starts recording with new chord
  result = reduce(result.state, { type: "HotkeyPressed" });
  assert.deepStrictEqual(result.state, { type: "Recording", chord: "Ctrl+Alt+L" });

  // Test full flow
  result = reduce({ type: "Idle", chord: "Ctrl+Alt+Space" }, { type: "HotkeyPressed" });
  result = reduce(result.state, { type: "RecordingStopped" });
  assert.deepStrictEqual(result.state, { type: "Processing", chord: "Ctrl+Alt+Space" });
  
  const candidates = [
    { text: "C1", source: "vsr" },
    { text: "C2", source: "llm" },
    { text: "C3", source: "vsr" },
    { text: "C4", source: "vsr" },
    { text: "C5", source: "vsr" },
    { text: "C6", source: "vsr" }
  ];
  result = reduce(result.state, { type: "ProcessingComplete", candidates, lowQuality: true, autoInsertThresholdMet: false });
  // Should truncate to 5
  assert.deepStrictEqual(result.state, { type: "ShowingCandidates", chord: "Ctrl+Alt+Space", candidates: candidates.slice(0, 5), lowQuality: true, autoInsertThresholdMet: false });
  
  // Select first candidate
  const selectResult = reduce(result.state, { type: "NumberKeyPressed", index: 1 });
  assert.deepStrictEqual(selectResult.state, { type: "Idle", chord: "Ctrl+Alt+Space" });
  assert.deepStrictEqual(selectResult.action, { type: "InsertCandidate", candidate: candidates[0] });

  // Test Escape from Candidates
  result = reduce({ type: "ShowingCandidates", chord: "Ctrl+Alt+Space", candidates: candidates.slice(0, 5), lowQuality: false, autoInsertThresholdMet: false }, { type: "EscapePressed" });
  assert.deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+Space" });
  assert.deepStrictEqual(result.action, { type: "Cancel" });

  // Test Escape from Recording
  result = reduce({ type: "Recording", chord: "Ctrl+Alt+L" }, { type: "EscapePressed" });
  assert.deepStrictEqual(result.state, { type: "Idle", chord: "Ctrl+Alt+L" });
  assert.deepStrictEqual(result.action, { type: "Cancel" });

  const container = new MockElement("ol");
  const maliciousCandidates = [
    { text: "<img src=x onerror=alert(1)>", source: "<b>llm</b>" }
  ];
  renderCandidates(container as any, maliciousCandidates);
  
  assert.strictEqual(container.innerHTML, "");
  assert.strictEqual(container.children.length, 1);
  
  const firstChild = container.children[0];
  assert.strictEqual(firstChild.tagName, "li");
  assert.strictEqual(firstChild.children.length, 5);
  
  assert.strictEqual(firstChild.children[0].textContent, "1.");
  assert.strictEqual(firstChild.children[2].textContent, "<img src=x onerror=alert(1)>");
  assert.strictEqual(firstChild.children[4].textContent, "<b>llm</b>");
}

runTests();
