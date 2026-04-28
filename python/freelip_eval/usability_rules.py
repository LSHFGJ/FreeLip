import re
from typing import List

def normalize_text(text: str) -> str:
    if not text:
        return ""
    text = re.sub(r'\s+', '', text)
    text = re.sub(r'[。，！？.,!?]$', '', text)
    return text

def is_usable(candidate: str, target_text: str, acceptable_equivalents: List[str]) -> bool:
    normalized_candidate = normalize_text(candidate)
    
    if normalized_candidate == normalize_text(target_text):
        return True
        
    for equiv in acceptable_equivalents:
        if normalized_candidate == normalize_text(equiv):
            return True
            
    return False

def check_top5_usability(candidates: List[str], target_text: str, acceptable_equivalents: List[str]) -> bool:
    for candidate in candidates[:5]:
        if is_usable(candidate, target_text, acceptable_equivalents):
            return True
    return False
