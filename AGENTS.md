# SharbCut development checks

- BMTS-lite is the default for the app, UI checks, Agent/timeline work, and generated-plan tests. Use a small real media set for end-to-end checks.
- Do not set `SHARBCUT_MONTAGE_TEST_FULL` during routine development. The full BGM-Montage pipeline is only for core-algorithm changes, full-mode compatibility, or formal pre-release regression.
- Both modes must import editable clips through the normal timeline command path and keep undo/redo working.
