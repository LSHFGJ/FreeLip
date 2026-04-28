# AI Prompt Short Chinese Evaluation Suite

## Labeling Rules
1. **Target Text (`target_text`)**: The exact intended prompt in simplified Chinese.
2. **Acceptable Equivalents (`acceptable_equivalents`)**: A list of semantically identical or highly similar phrases that do not change the AI's intent. 
3. **Punctuation**: Punctuation-only differences (e.g., full-width vs. half-width, or presence/absence of trailing periods) should be considered acceptable equivalents if not already in the list.
4. **Usability**: A prediction is "Usable" (Top-5 rule) if it matches the target text or any acceptable equivalent exactly.

## License Note
This metadata contains text-label fixtures for internal evaluation purposes. No media files are included in this suite. Media must be mapped from accessible public datasets or video sources separately.
