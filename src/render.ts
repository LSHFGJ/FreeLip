import type { OverlayCandidate } from "./hotkeyState.ts";

export function renderCandidates(container: Element, candidates: OverlayCandidate[]) {
  container.innerHTML = "";
  candidates.forEach((c, i) => {
    const li = document.createElement("li");
    li.className = "candidate-item";
    li.setAttribute("data-index", String(i));
    
    const numSpan = document.createElement("span");
    numSpan.className = "candidate-num";
    numSpan.textContent = `${i + 1}.`;
    
    const spaceNode = document.createTextNode(" ");
    
    const textSpan = document.createElement("span");
    textSpan.className = "candidate-text";
    textSpan.textContent = c.text;
    
    const spaceNode2 = document.createTextNode(" ");

    const sourceSpan = document.createElement("span");
    sourceSpan.className = "candidate-source";
    sourceSpan.textContent = c.source;
    
    li.appendChild(numSpan);
    li.appendChild(spaceNode);
    li.appendChild(textSpan);
    li.appendChild(spaceNode2);
    li.appendChild(sourceSpan);
    
    container.appendChild(li);
  });
}
