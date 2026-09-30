// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 SharbCut contributors

//! Imports an external edit-decision list as ordinary, undoable timeline clips.
//! No renderer output is imported: every shot still points to its source media.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use concat_project::Command;
use concat_project::commands::ClipPatch;
use concat_project::model::{Crop, Project, Transition};
use serde::Deserialize;

use crate::media;

#[derive(Deserialize)]
struct Decisions {
    shots: Vec<Shot>,
    #[serde(default)]
    audio_tracks: Vec<AudioDecision>,
}

#[derive(Deserialize)]
struct Shot {
    source_path: String,
    source_start: f64,
    timeline_start: f64,
    duration: f64,
    #[serde(default = "unity")]
    speed: f64,
    #[serde(default)]
    transition_out: Option<String>,
    #[serde(default)]
    transition_duration_seconds: Option<f64>,
    #[serde(default)]
    transform: Option<ShotTransform>,
}

#[derive(Deserialize)]
struct ShotTransform {
    #[serde(default)]
    crop: Option<CropDecision>,
}

#[derive(Deserialize)]
struct CropDecision {
    mode: String,
    #[serde(default)]
    crop_rect_norm: Option<[f64; 4]>,
}

#[derive(Deserialize)]
struct AudioDecision {
    role: String,
    source_path: String,
    #[serde(default)]
    source_start: f64,
    #[serde(default)]
    timeline_start: f64,
    duration: f64,
    #[serde(default = "unity")]
    volume: f64,
}

fn unity() -> f64 {
    1.0
}

fn stage_references(library: &Path, destination: &Path) -> Result<(), String> {
    let mut pending = vec![library.to_owned()];
    let mut videos = Vec::new();
    while let Some(folder) = pending.pop() {
        for entry in std::fs::read_dir(&folder).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        [
                            "mp4", "mov", "m4v", "mkv", "webm", "avi", "wmv", "flv", "mts", "m2ts",
                            "ts",
                        ]
                        .contains(&extension.to_ascii_lowercase().as_str())
                    })
            {
                videos.push(path);
            }
        }
    }
    videos.sort();
    if videos.is_empty() {
        return Err("The selected footage folder contains no videos.".to_owned());
    }
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(destination).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path().is_file()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("reference-")
        {
            std::fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    // ponytail: three stable library samples cap first-run analysis; expose a reference picker if style control becomes necessary.
    for (index, video) in videos.into_iter().take(3).enumerate() {
        let extension = video
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("mp4");
        let reference = destination.join(format!("reference-{index}.{extension}"));
        std::fs::hard_link(&video, &reference)
            .or_else(|_| std::fs::copy(&video, &reference).map(|_| ()))
            .map_err(|error| {
                format!("Cannot stage reference video {}: {error}", video.display())
            })?;
    }
    Ok(())
}

/// One import prepared off the UI thread, ready for `Session::apply`.
pub struct PreparedMontage {
    /// One atomic history entry, including any newly discovered media.
    pub command: Command,
    /// Number of placed picture cuts.
    pub shots: usize,
    /// Number of placed BGM clips.
    pub bgm_tracks: usize,
}

/// The quick audio-led planner is the normal editing path; full analysis is opt-in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Fast audio-led planning without full media analysis or rendering.
    Lite,
    /// The original BGM Montage v1.4.6 pipeline.
    Full,
}

/// Inputs for the pinned BGM Montage v1.4.6 local-library pipeline.
pub struct Generation {
    /// BMTS Lite by default, full v1.4.6 only on explicit request.
    pub mode: Mode,
    /// Project-local Python interpreter with the locked BGM dependencies.
    pub python: PathBuf,
    /// The pinned v1.4.6 entry point.
    pub script: PathBuf,
    /// The pinned BMTS Lite source and thin SharbCut plan adapter.
    pub lite_root: PathBuf,
    /// SharbCut adapter that emits ordinary editable decisions.
    pub lite_script: PathBuf,
    /// Directory containing ffmpeg.exe and ffprobe.exe.
    pub ffmpeg_bin: PathBuf,
    /// SharbCut project folder, which receives the run and cache.
    pub project_dir: PathBuf,
    /// User-selected song.
    pub bgm: PathBuf,
    /// User-selected read-only footage library.
    pub library: PathBuf,
    /// Imported project videos for an Agent request; Lite mode only.
    pub sources: Vec<PathBuf>,
    /// Visual direction.
    pub theme: String,
    /// Requested output seconds.
    pub duration: f64,
    /// Output frame, e.g. `1920x1080`.
    pub ratio: String,
}

/// Run the mature planner/QA pipeline; the caller then passes its decisions
/// to [`prepare`] and applies them to the live timeline.
pub fn generate(input: Generation) -> Result<PathBuf, String> {
    if !input.python.is_file()
        || !input.ffmpeg_bin.join("ffmpeg.exe").is_file()
        || match input.mode {
            Mode::Lite => {
                !input.lite_root.join("worker.py").is_file() || !input.lite_script.is_file()
            }
            Mode::Full => !input.script.is_file(),
        }
    {
        return Err("BGM Montage runtime is missing. Run scripts/setup-dev.ps1.".to_owned());
    }
    if !input.bgm.is_file()
        || (input.sources.is_empty() && !input.library.is_dir())
        || (input.mode == Mode::Full && (!input.library.is_dir() || !input.sources.is_empty()))
        || input.sources.iter().any(|source| !source.is_file())
    {
        return Err("Select an existing BGM file and footage.".to_owned());
    }
    let duration = media::probe(&input.bgm.to_string_lossy())?
        .duration
        .unwrap_or(input.duration)
        .min(input.duration);
    if !duration.is_finite() || duration < 5.0 || input.theme.trim().is_empty() {
        return Err("Montage needs at least five seconds and a visual direction.".to_owned());
    }
    let run_id = format!(
        "sharbcut-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis(),
        std::process::id()
    );
    let output_dir = input.project_dir.join("bgm-montage");
    let cache_dir = input.project_dir.join("cache").join("bgm-montage");
    let temp_dir = cache_dir.join("tmp");
    let reference_dir = cache_dir.join("reference-input");
    let material_dir = cache_dir.join("material");
    std::fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&temp_dir).map_err(|error| error.to_string())?;
    if input.mode == Mode::Lite {
        return generate_lite(
            &input,
            duration,
            &run_id,
            &output_dir,
            &cache_dir,
            &temp_dir,
        );
    }
    std::fs::create_dir_all(&reference_dir).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&material_dir).map_err(|error| error.to_string())?;
    stage_references(&input.library, &reference_dir)?;
    for name in ["huggingface", "torch", "numba"] {
        std::fs::create_dir_all(cache_dir.join(name)).map_err(|error| error.to_string())?;
    }
    let path = std::env::join_paths(
        std::iter::once(input.ffmpeg_bin.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.into_os_string()),
        ),
    )
    .map_err(|error| error.to_string())?;
    let mut process = Process::new(&input.python);
    process
        .arg(&input.script)
        .arg("--source-provider")
        .arg("local-library")
        .arg("--project-name")
        .arg("sharbcut")
        .arg("--reference-dir")
        .arg(&reference_dir)
        .arg("--material-dir")
        .arg(&material_dir)
        .arg("--local-library-dir")
        .arg(&input.library)
        .arg("--bgm")
        .arg(&input.bgm)
        .arg("--theme")
        .arg(&input.theme)
        .arg("--duration")
        .arg(duration.to_string())
        .arg("--ratio")
        .arg(&input.ratio)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--cache-dir")
        .arg(&cache_dir)
        .arg("--run-id")
        .arg(&run_id)
        .arg("--agent-visual-review")
        .arg("off")
        .arg("--allow-semantic-fallback")
        .env("PATH", path)
        .env("TEMP", &temp_dir)
        .env("TMP", &temp_dir)
        .env("HF_HOME", cache_dir.join("huggingface"))
        .env("TORCH_HOME", cache_dir.join("torch"))
        .env("NUMBA_CACHE_DIR", cache_dir.join("numba"))
        .env("BGM_MONTAGE_PROJECT_ROOT", &input.project_dir)
        .env("BGM_MONTAGE_LIBRARY_ROOT", cache_dir.join("shared-library"))
        .env("BGM_MONTAGE_SEMANTIC_OFFLINE", "1");
    #[cfg(test)]
    if std::env::var_os("SHARBCUT_MONTAGE_TEST_FAST").is_some() {
        process
            .arg("--assets")
            .arg("24")
            .arg("--max-reuse-per-asset")
            .arg("4");
    }
    let output = process
        .output()
        .map_err(|error| format!("Cannot start BGM Montage: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        let tail = error
            .chars()
            .rev()
            .take(2000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        return Err(format!("BGM Montage failed: {tail}"));
    }
    let decisions = output_dir
        .join("sharbcut")
        .join(run_id)
        .join("edit_decisions.json");
    if !decisions.is_file() {
        return Err(format!(
            "BGM Montage produced no edit decisions: {}",
            decisions.display()
        ));
    }
    Ok(decisions)
}

fn generate_lite(
    input: &Generation,
    duration: f64,
    run_id: &str,
    output_dir: &Path,
    cache_dir: &Path,
    temp_dir: &Path,
) -> Result<PathBuf, String> {
    let decisions = output_dir
        .join("bmts-lite")
        .join(run_id)
        .join("edit_decisions.json");
    std::fs::create_dir_all(cache_dir.join("numba")).map_err(|error| error.to_string())?;
    let path = std::env::join_paths(
        std::iter::once(input.ffmpeg_bin.as_os_str().to_owned()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|part| part.into_os_string()),
        ),
    )
    .map_err(|error| error.to_string())?;
    let mut process = Process::new(&input.python);
    process
        .arg(&input.lite_script)
        .arg("--lite-root")
        .arg(&input.lite_root)
        .arg("--bgm")
        .arg(&input.bgm)
        .arg("--duration")
        .arg(duration.to_string())
        .arg("--cache-dir")
        .arg(cache_dir.join("lite"))
        .arg("--output")
        .arg(&decisions)
        .env("PATH", path)
        .env("TEMP", temp_dir)
        .env("TMP", temp_dir)
        .env("NUMBA_CACHE_DIR", cache_dir.join("numba"));
    if input.sources.is_empty() {
        process.arg("--library").arg(&input.library);
    } else {
        for source in &input.sources {
            process.arg("--source").arg(source);
        }
    }
    let output = process
        .output()
        .map_err(|error| format!("Cannot start BMTS Lite: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "BMTS Lite failed: {}",
            error
                .chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    if !decisions.is_file() {
        return Err("BMTS Lite produced no edit decisions.".to_owned());
    }
    Ok(decisions)
}

fn source_path(plan: &Path, name: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(name);
    let candidate = if candidate.is_absolute() {
        candidate.to_owned()
    } else {
        plan.parent().unwrap_or(Path::new(".")).join(candidate)
    };
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("Montage source {}: {error}", candidate.display()))?;
    if !resolved.is_file() {
        return Err(format!(
            "Montage source is not a file: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn valid_span(start: f64, source_start: f64, duration: f64, speed: f64, volume: f64) -> bool {
    start.is_finite()
        && start >= 0.0
        && source_start.is_finite()
        && source_start >= 0.0
        && duration.is_finite()
        && duration >= 1.0 / 60.0
        && speed.is_finite()
        && (0.0625..=16.0).contains(&speed)
        && volume.is_finite()
        && volume >= 0.0
        && (start + duration).is_finite()
        && (source_start + duration * speed).is_finite()
}

fn transition_before(shots: &[Shot], index: usize) -> Option<Transition> {
    let previous = shots.get(index.checked_sub(1)?)?;
    let current = shots.get(index)?;
    let kind = previous.transition_out.as_deref()?;
    let (id, default) = match kind {
        "dissolve" => ("cross-fade", 0.22),
        "fade_through_black" => ("fade-black", 0.16),
        "hard_cut" => return None,
        _ => return None,
    };
    let requested = previous.transition_duration_seconds.unwrap_or(default);
    if !requested.is_finite() || requested <= 0.0 {
        return None;
    }
    Some(Transition {
        id: id.to_owned(),
        duration: requested
            .min(previous.duration * 0.22)
            .min(current.duration * 0.22)
            .clamp(0.06, 0.30),
    })
}

fn shot_crop(shot: &Shot) -> Option<Crop> {
    let crop = shot.transform.as_ref()?.crop.as_ref()?;
    if crop.mode != "subject_crop" {
        return None;
    }
    let [left, top, right, bottom] = crop.crop_rect_norm?;
    if ![left, top, right, bottom]
        .iter()
        .all(|value| value.is_finite())
        || right <= left
        || bottom <= top
    {
        return None;
    }
    Some(
        Crop {
            left,
            top,
            right: 1.0 - right,
            bottom: 1.0 - bottom,
        }
        .tidy(),
    )
}

/// Probe every referenced source and append its editable decisions after the
/// current timeline's end. A failed probe leaves the project unchanged.
pub fn prepare(path: &Path, project: &Project) -> Result<PreparedMontage, String> {
    let json = std::fs::read_to_string(path)
        .map_err(|error| format!("Cannot read montage decisions: {error}"))?;
    let decisions: Decisions = serde_json::from_str(&json)
        .map_err(|error| format!("Invalid montage decisions: {error}"))?;
    if decisions.shots.is_empty() {
        return Err("Montage decisions contain no shots.".to_owned());
    }

    let timeline = project.active();
    let video_track = timeline
        .tracks
        .first()
        .ok_or("The timeline has no tracks.")?
        .id
        .clone();
    let offset = timeline
        .clips
        .iter()
        .map(|clip| clip.start + clip.duration)
        .fold(0.0_f64, f64::max);
    let mut commands = Vec::new();
    let mut seen = HashSet::new();
    let mut add_segment = |source: &str,
                           track_id: Option<String>,
                           start: f64,
                           source_start: f64,
                           duration: f64,
                           speed: f64,
                           patch: ClipPatch|
     -> Result<(), String> {
        if !valid_span(
            start,
            source_start,
            duration,
            speed,
            patch.volume.unwrap_or(1.0),
        ) {
            return Err(format!("Invalid montage cut for {source}"));
        }
        let resolved = source_path(path, source)?;
        let existing = project.media.iter().find(|item| {
            Path::new(&item.path).canonicalize().ok().as_deref() == Some(resolved.as_path())
        });
        let media_path = existing
            .map(|item| item.path.clone())
            .unwrap_or_else(|| resolved.to_string_lossy().into_owned());
        if seen.insert(media_path.clone()) && existing.is_none() {
            commands.push(Command::AddMedia {
                item: media::probe(&media_path)?.to_new_media(),
            });
        }
        commands.push(Command::AddClipSegment {
            media_path,
            track_id,
            start: offset + start,
            source_start,
            duration,
            speed,
            patch,
        });
        Ok(())
    };

    for (index, shot) in decisions.shots.iter().enumerate() {
        let crop = shot_crop(shot);
        let transition = transition_before(&decisions.shots, index);
        add_segment(
            &shot.source_path,
            Some(video_track.clone()),
            shot.timeline_start,
            shot.source_start,
            shot.duration,
            shot.speed,
            ClipPatch {
                volume: Some(0.0),
                crop: crop.map(Some),
                transition_in: transition.map(Some),
                ..Default::default()
            },
        )?;
    }

    let mut bgm_tracks = 0;
    for audio in &decisions.audio_tracks {
        if audio.role != "bgm" {
            continue;
        }
        add_segment(
            &audio.source_path,
            None,
            audio.timeline_start,
            audio.source_start,
            audio.duration,
            1.0,
            ClipPatch {
                volume: Some(audio.volume),
                fade_in: Some((audio.duration * 0.04).min(0.35)),
                fade_out: Some((audio.duration * 0.08).min(1.2)),
                ..Default::default()
            },
        )?;
        bgm_tracks += 1;
    }
    if bgm_tracks > 0 && timeline.tracks.len() == 1 {
        commands.insert(0, Command::AddTrack);
    }
    Ok(PreparedMontage {
        command: Command::Batch { commands },
        shots: decisions.shots.len(),
        bgm_tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_project::Editor;

    #[test]
    fn rejects_bad_spans_before_editing() {
        assert!(valid_span(0.0, 1.0, 2.0, 1.0, 0.5));
        assert!(!valid_span(f64::NAN, 1.0, 2.0, 1.0, 0.5));
        assert!(!valid_span(0.0, 1.0, 0.0, 1.0, 0.5));
        assert!(!valid_span(0.0, 1.0, 2.0, 100.0, 0.5));
        assert!(!valid_span(0.0, 1.0, 2.0, 1.0, -1.0));
    }

    #[test]
    fn maps_v146_transition_and_subject_crop() {
        let plan: Decisions = serde_json::from_str(r#"{"shots":[{"source_path":"a.mp4","source_start":0,"timeline_start":0,"duration":1,"transition_out":"dissolve","transition_duration_seconds":0.2},{"source_path":"b.mp4","source_start":0,"timeline_start":1,"duration":1,"transform":{"crop":{"mode":"subject_crop","crop_rect_norm":[0.1,0.2,0.9,0.8]}}}]}"#).expect("v1.4.6 decisions");
        let transition = transition_before(&plan.shots, 1).expect("transition");
        assert_eq!(
            (transition.id.as_str(), transition.duration),
            ("cross-fade", 0.2)
        );
        let crop = shot_crop(&plan.shots[1]).expect("crop");
        assert!((crop.left - 0.1).abs() < 1e-9);
        assert!((crop.bottom - 0.2).abs() < 1e-9);
    }

    #[test]
    fn imports_source_backed_cuts_with_one_undo() {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let folder = base.join(format!("montage-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("test folder");
        let media = folder.join("tone.wav");
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + 16_000).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_000_u32.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.resize(wav.len() + 16_000, 0);
        std::fs::write(&media, wav).expect("test media");
        let plan = folder.join("edit_decisions.json");
        std::fs::write(
            &plan,
            r#"{"schema_version":"1.3","shots":[{"source_path":"tone.wav","local_path":"tone.wav","source_start":0,"timeline_start":0,"output_start":0,"duration":0.5,"output_duration":0.5,"speed":1}],"audio_tracks":[{"role":"bgm","source_path":"tone.wav","timeline_start":0,"duration":0.5}]}"#,
        )
        .expect("test plan");
        let mut editor = Editor::new();
        let prepared = prepare(&plan, editor.project()).expect("prepared");
        assert_eq!((prepared.shots, prepared.bgm_tracks), (1, 1));
        editor.apply(prepared.command).expect("applied");
        assert_eq!(editor.project().media.len(), 1);
        assert_eq!(editor.project().active().clips.len(), 2);
        assert_ne!(
            editor.project().active().clips[0].track_id,
            editor.project().active().clips[1].track_id
        );
        assert!(editor.undo());
        assert!(editor.project().active().clips.is_empty());
        assert!(editor.redo());
        assert_eq!(editor.project().active().clips.len(), 2);
        std::fs::remove_dir_all(folder).expect("test cleanup");
    }

    #[test]
    fn provided_plan_smoke() {
        let Ok(file) = std::env::var("SHARBCUT_MONTAGE_TEST_PLAN") else {
            return;
        };
        let mut editor = Editor::new();
        let prepared = prepare(Path::new(&file), editor.project()).expect("real plan prepares");
        let expected = prepared.shots + prepared.bgm_tracks;
        editor.apply(prepared.command).expect("real plan imports");
        assert_eq!(editor.project().active().clips.len(), expected);
        assert!(editor.undo());
        assert!(editor.project().active().clips.is_empty());
        assert!(editor.redo());
        assert_eq!(editor.project().active().clips.len(), expected);
    }

    #[test]
    fn generated_plan_smoke() {
        let (Ok(bgm), Ok(library)) = (
            std::env::var("SHARBCUT_MONTAGE_TEST_BGM"),
            std::env::var("SHARBCUT_MONTAGE_TEST_LIBRARY"),
        ) else {
            return;
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root");
        let (python, script, lite_root, lite_script, ffmpeg_bin) =
            if let Some(installed) = std::env::var_os("SHARBCUT_MONTAGE_TEST_RUNTIME_ROOT") {
                let installed = PathBuf::from(installed).join("bgm-runtime");
                (
                    installed.join("python/python.exe"),
                    installed.join("bgm-montage/scripts/bgm_montage.py"),
                    installed.join("bmts-lite"),
                    installed.join("bmts-lite-plan.py"),
                    installed.join("ffmpeg/bin"),
                )
            } else {
                (
                    root.join(".tools/bgm-venv/Scripts/python.exe"),
                    root.join("vendor/bgm-montage/scripts/bgm_montage.py"),
                    root.join("vendor/bmts-lite"),
                    root.join("scripts/bmts-lite-plan.py"),
                    root.join("vendor/ffmpeg-n8.1-latest-win64-gpl-shared-8.1/bin"),
                )
            };
        let sources = if std::env::var_os("SHARBCUT_MONTAGE_TEST_SOURCES").is_some() {
            let mut files = std::fs::read_dir(&library)
                .expect("test library")
                .map(|entry| entry.expect("entry").path())
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            files.sort();
            files.truncate(6);
            files
        } else {
            Vec::new()
        };
        let plan = generate(Generation {
            mode: if std::env::var_os("SHARBCUT_MONTAGE_TEST_FULL").is_some() {
                Mode::Full
            } else {
                Mode::Lite
            },
            python,
            script,
            lite_root,
            lite_script,
            ffmpeg_bin,
            project_dir: root.join(".tools/montage-integration-test"),
            bgm: bgm.into(),
            library: if sources.is_empty() {
                library.into()
            } else {
                PathBuf::new()
            },
            sources,
            theme: std::env::var("SHARBCUT_MONTAGE_TEST_THEME")
                .unwrap_or_else(|_| "Iceland cinematic landscape".to_owned()),
            duration: std::env::var("SHARBCUT_MONTAGE_TEST_DURATION")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(5.0),
            ratio: "1920x1080".to_owned(),
        })
        .expect("montage generates a plan");
        let mut editor = Editor::new();
        let prepared = prepare(&plan, editor.project()).expect("generated plan prepares");
        editor
            .apply(prepared.command)
            .expect("generated plan imports");
        assert!(!editor.project().active().clips.is_empty());
        assert!(editor.undo());
        assert!(editor.project().active().clips.is_empty());
        assert!(editor.redo());
        assert!(!editor.project().active().clips.is_empty());
    }
}
