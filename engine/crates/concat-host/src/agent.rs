// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 SharbCut contributors

//! A model proposes ordinary project commands; only a small edit allowlist
//! crosses back into the timeline. The UI applies the returned Batch once.

use std::io::Read;
use std::path::PathBuf;

use base64::Engine;
use concat_media::{AudioDecoder, AudioOptions, SampleFormat};
use concat_project::commands::{ClipPatch, Command, IdMint, apply};
use concat_project::model::{MediaItem, MediaKind, Project, TextStyle};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::media;

/// User-selected OpenAI-compatible endpoint and credential.
pub struct Config {
    /// Endpoint prefix, usually ending in `/v1`.
    pub base_url: String,
    /// Model identifier understood by that endpoint.
    pub model: String,
    /// Per-user secret, never stored in the project.
    pub api_key: String,
    /// Optional Chat Completions reasoning effort; omitted for older endpoints.
    pub reasoning_effort: Option<String>,
}

/// A checked model proposal, ready to apply as one undo step.
pub struct Plan {
    /// Text shown to the user.
    pub reply: String,
    /// None when the model requested no direct timeline command.
    pub command: Option<Command>,
    /// Optional fast beat-edit request over media already in the project.
    pub montage: Option<Montage>,
    /// Number of contained operations.
    pub edits: usize,
}

/// Checked inputs for the normal BMTS-lite timeline path.
pub struct Montage {
    /// Imported audio source selected by the model.
    pub bgm: PathBuf,
    /// Imported video sources, sampled to the fast planner's limit.
    pub videos: Vec<PathBuf>,
    /// Requested timeline length in seconds.
    pub duration: f64,
}

#[derive(Deserialize)]
struct ModelReply {
    reply: String,
    #[serde(default)]
    commands: Vec<Value>,
    #[serde(default)]
    montage: Option<ModelMontage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelMontage {
    bgm_media_id: String,
    #[serde(default)]
    video_media_ids: Vec<String>,
    #[serde(default = "montage_duration")]
    duration: f64,
}

fn montage_duration() -> f64 {
    30.0
}

const SYSTEM: &str = r#"You are SharbCut's timeline editing assistant. Reply with ONE JSON object only: {"reply":"short Chinese explanation","commands":[...]}. Commands are SharbCut's existing camelCase command format. Allowed ops: removeClips {clipIds}, splitClips {clipIds,time}, trimClip {clipId,edge:"start"|"end",delta}, moveClips {moves:[{clipId,start,trackId}]}, setClipSpeed {clipId,speed}, updateClip {clipId,patch:{volume?,fadeIn?,fadeOut?,transitionIn?:{id:"cross-fade"|"fade-black"|"fade-white"|"push"|"zoom"|"wipe-left"|"wipe-right",duration:seconds}}}, addClipSegment {mediaPath,trackId?,start,sourceStart,duration,speed,patch:{}}, addTextClip {trackId?,start,duration,style:{content}}. Use ONLY ids and media paths from the provided project. Times are seconds. To make a rough cut, remove existing clips and add source-backed segments in one response. To place a BGM already in the bin, use addClipSegment on its path, with an audio track. For a request to automatically cut footage to a BGM, return {"reply":"将使用 BMTS-lite 自动卡点","commands":[],"montage":{"bgmMediaId":"existing audio media id","videoMediaIds":["existing video media ids"],"duration":30}}. Omit videoMediaIds to use the imported videos; never include montage and commands together. Montage defaults to fast BMTS-lite, not full analysis. Some requests include sparse labeled preview frames. Use only those frames as visual evidence; they do not show the whole video. Audio energy numbers indicate loudness, not speech or lyrics. An audio transcript, when provided, covers only the first 15 seconds of one selected clip and may contain recognition errors. Existing text clips may contain captions. If no previews are attached, do not claim to have seen the footage. If the request needs content analysis unavailable in the context, explain the limitation and return no commands. Never invent source files or claim to have heard speech not present in a transcript. No markdown fences."#;

fn validate(command: &Command, project: &Project) -> Result<(), String> {
    let timeline = project.active();
    let clip = |id: &str| timeline.clips.iter().any(|clip| clip.id == id);
    let track = |id: &str| timeline.tracks.iter().any(|track| track.id == id);
    let finite = |value: f64| value.is_finite();
    match command {
        Command::RemoveClips { clip_ids } | Command::SplitClips { clip_ids, .. } => {
            if clip_ids.is_empty() || clip_ids.iter().any(|id| !clip(id)) {
                return Err("AI referenced a missing clip.".to_owned());
            }
            if let Command::SplitClips { time, .. } = command {
                if !finite(*time) || *time < 0.0 {
                    return Err("AI provided an invalid cut time.".to_owned());
                }
            }
        }
        Command::TrimClip { clip_id, delta, .. } => {
            if !clip(clip_id) || !finite(*delta) {
                return Err("AI provided an invalid trim.".to_owned());
            }
        }
        Command::MoveClips { moves } => {
            if moves.is_empty()
                || moves.iter().any(|item| {
                    !clip(&item.clip_id)
                        || !track(&item.track_id)
                        || !finite(item.start)
                        || item.start < 0.0
                })
            {
                return Err("AI provided an invalid move.".to_owned());
            }
        }
        Command::SetClipSpeed { clip_id, speed } => {
            if !clip(clip_id) || !finite(*speed) || !(0.0625..=16.0).contains(speed) {
                return Err("AI provided an invalid speed.".to_owned());
            }
        }
        Command::UpdateClip { clip_id, patch } => {
            if !clip(clip_id)
                || patch
                    .volume
                    .is_some_and(|value| !finite(value) || value < 0.0)
                || patch
                    .fade_in
                    .is_some_and(|value| !finite(value) || value < 0.0)
                || patch
                    .fade_out
                    .is_some_and(|value| !finite(value) || value < 0.0)
                || patch
                    .transition_in
                    .as_ref()
                    .and_then(Option::as_ref)
                    .is_some_and(|value| {
                        !finite(value.duration)
                            || !(0.0..=2.0).contains(&value.duration)
                            || ![
                                "cross-fade",
                                "fade-black",
                                "fade-white",
                                "push",
                                "zoom",
                                "wipe-left",
                                "wipe-right",
                            ]
                            .contains(&value.id.as_str())
                    })
                || *patch
                    != (ClipPatch {
                        volume: patch.volume,
                        fade_in: patch.fade_in,
                        fade_out: patch.fade_out,
                        transition_in: patch.transition_in.clone(),
                        ..Default::default()
                    })
            {
                return Err("AI provided an unsupported clip change.".to_owned());
            }
        }
        Command::AddClipSegment {
            media_path,
            track_id,
            start,
            source_start,
            duration,
            speed,
            patch,
        } => {
            let media = project.media.iter().find(|media| media.path == *media_path);
            if media.is_none()
                || track_id.as_ref().is_some_and(|id| !track(id))
                || !finite(*start)
                || *start < 0.0
                || !finite(*source_start)
                || *source_start < 0.0
                || !finite(*duration)
                || *duration < 1.0 / 60.0
                || !finite(*speed)
                || !(0.0625..=16.0).contains(speed)
                || media
                    .and_then(|media| media.duration)
                    .is_some_and(|total| source_start + duration * speed > total + 0.05)
                || *patch != ClipPatch::default()
            {
                return Err("AI provided an invalid source-backed segment.".to_owned());
            }
        }
        Command::AddTextClip {
            track_id,
            start,
            duration,
            style,
            offset_y,
        } => {
            if track_id.as_ref().is_some_and(|id| !track(id))
                || !finite(*start)
                || *start < 0.0
                || duration.is_some_and(|value| !finite(value) || value < 1.0 / 60.0)
                || offset_y.is_some_and(|value| !finite(value))
                || style.as_ref().is_none_or(|style| {
                    style.content.trim().is_empty()
                        || *style
                            != (TextStyle {
                                content: style.content.clone(),
                                ..Default::default()
                            })
                })
            {
                return Err("AI provided an invalid title.".to_owned());
            }
        }
        _ => return Err("AI requested an unsupported project operation.".to_owned()),
    }
    Ok(())
}

/// Decode, allowlist and dry-run a model response against a project snapshot.
pub fn parse(content: &str, project: &Project) -> Result<Plan, String> {
    let content = content.trim();
    let content = content.strip_prefix("```json").unwrap_or(content);
    let content = content.strip_suffix("```").unwrap_or(content).trim();
    let response: ModelReply = serde_json::from_str(content)
        .map_err(|error| format!("AI response is not a valid edit plan: {error}"))?;
    if response.commands.len() > 200 {
        return Err("AI edit plan is too large.".to_owned());
    }
    if response.montage.is_some() && !response.commands.is_empty() {
        return Err("AI must request a montage or timeline edits, not both.".to_owned());
    }
    let montage = if let Some(request) = response.montage {
        if !request.duration.is_finite() || !(5.0..=600.0).contains(&request.duration) {
            return Err("AI provided an invalid montage duration.".to_owned());
        }
        let bgm = project
            .media
            .iter()
            .find(|item| item.id == request.bgm_media_id)
            .filter(|item| {
                item.kind == MediaKind::Audio && !item.placeholder && !item.path.is_empty()
            })
            .ok_or("AI referenced a missing BGM in the media bin.")?;
        let mut videos = Vec::new();
        if request.video_media_ids.is_empty() {
            videos.extend(
                project
                    .media
                    .iter()
                    .filter(|item| {
                        item.kind == MediaKind::Video && !item.placeholder && !item.path.is_empty()
                    })
                    .map(|item| PathBuf::from(&item.path)),
            );
        } else {
            for id in request.video_media_ids {
                let video = project
                    .media
                    .iter()
                    .find(|item| item.id == id)
                    .filter(|item| {
                        item.kind == MediaKind::Video && !item.placeholder && !item.path.is_empty()
                    })
                    .ok_or("AI referenced a missing video in the media bin.")?;
                let path = PathBuf::from(&video.path);
                if !videos.contains(&path) {
                    videos.push(path);
                }
            }
        }
        if videos.is_empty() {
            return Err("Import video footage before requesting a BGM montage.".to_owned());
        }
        if videos.len() > 24 {
            videos = (0..24)
                .map(|index| videos[index * videos.len() / 24].clone())
                .collect();
        }
        Some(Montage {
            bgm: bgm.path.clone().into(),
            videos,
            duration: request.duration,
        })
    } else {
        None
    };
    let mut commands = Vec::with_capacity(response.commands.len());
    for value in response.commands {
        let command: Command = serde_json::from_value(value)
            .map_err(|error| format!("AI edit command is invalid: {error}"))?;
        validate(&command, project)?;
        commands.push(command);
    }
    let edits = commands.len() + usize::from(montage.is_some());
    let command = if commands.is_empty() {
        None
    } else {
        let command = Command::Batch { commands };
        let mut staged = project.clone();
        let mut mint = IdMint::default();
        mint.adopt_project(&staged);
        apply(&mut staged, &mut mint, command.clone())
            .map_err(|error| format!("AI edit plan cannot be applied: {error}"))?;
        Some(command)
    };
    Ok(Plan {
        reply: response.reply,
        command,
        montage,
        edits,
    })
}

fn preview_parts(project: &Project, selected: &[String]) -> Vec<Value> {
    let mut candidates: Vec<(&MediaItem, f64, String)> = project
        .active()
        .clips
        .iter()
        .filter(|clip| selected.contains(&clip.id) && clip.kind.is_visual())
        .filter_map(|clip| {
            project
                .media
                .iter()
                .find(|item| item.id == clip.media_id)
                .map(|item| {
                    (
                        item,
                        if item.kind == MediaKind::Image {
                            0.0
                        } else {
                            clip.source_start + clip.duration * clip.speed * 0.5
                        },
                        format!("clip {}", clip.id),
                    )
                })
        })
        .take(4)
        .collect();
    if candidates.is_empty() {
        let visuals: Vec<_> = project
            .media
            .iter()
            .filter(|item| {
                matches!(item.kind, MediaKind::Video | MediaKind::Image)
                    && !item.placeholder
                    && !item.path.is_empty()
            })
            .collect();
        let count = visuals.len().min(4);
        for index in 0..count {
            let item = visuals[index * visuals.len() / count];
            let time = if item.kind == MediaKind::Image {
                0.0
            } else {
                item.duration.unwrap_or(4.0) * 0.25
            };
            candidates.push((item, time, format!("media {}", item.id)));
        }
    }
    let mut parts = Vec::new();
    // ponytail: four sparse 320px frames bound latency and token cost; add deliberate deep analysis when needed.
    for (item, time, label) in candidates {
        if item.placeholder || item.path.is_empty() {
            continue;
        }
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        let Ok(frame) = media::still_at(&item.path, time, 320) else {
            continue;
        };
        let Ok(jpeg) = concat_media::jpeg(&frame, 4) else {
            continue;
        };
        if jpeg.len() > 128 * 1024 {
            continue;
        }
        parts.push(json!({"type":"text","text":format!("Preview of {label} ({}, {:.1}s)", item.name, time)}));
        parts.push(json!({"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(jpeg)),"detail":"low"}}));
    }
    parts
}

fn audio_levels(path: &str, start: f64, duration: f64, stream: Option<u32>) -> Option<Vec<f64>> {
    if !start.is_finite() || start < 0.0 || !duration.is_finite() || duration < 0.2 {
        return None;
    }
    let mut decoder = AudioDecoder::open(
        path,
        &AudioOptions {
            start: Some(start),
            duration: Some(duration.min(15.0)),
            rate: 8_000,
            channels: 1,
            format: SampleFormat::F32,
            stream: stream.map(|index| index as usize),
            ..AudioOptions::default()
        },
    )
    .ok()?;
    let samples = decoder.collect_f32().ok()?;
    if samples.is_empty() {
        return None;
    }
    Some(
        samples
            .chunks(16_000)
            .map(|chunk| {
                let power = chunk
                    .iter()
                    .map(|sample| f64::from(*sample).powi(2))
                    .sum::<f64>()
                    / chunk.len() as f64;
                (power.sqrt() * 1000.0).round() / 1000.0
            })
            .collect(),
    )
}

fn selected_audio(project: &Project, selected: &[String]) -> Vec<Value> {
    project
        .active()
        .clips
        .iter()
        .filter(|clip| selected.contains(&clip.id))
        .filter_map(|clip| {
            let item = project.media.iter().find(|item| item.id == clip.media_id)?;
            if !item.has_audio || item.placeholder {
                return None;
            }
            let levels = audio_levels(
                &item.path,
                clip.source_start,
                clip.duration * clip.speed,
                clip.audio_stream,
            )?;
            Some(json!({"clipId":clip.id,"sourceStart":clip.source_start,"bucketSeconds":2,"rms":levels}))
        })
        .take(2)
        .collect()
}

/// Ask the configured model to edit the current timeline off the UI thread.
pub fn request(
    config: Config,
    prompt: &str,
    project: &Project,
    selected: &[String],
    transcript: Option<&str>,
) -> Result<Plan, String> {
    let base = config.base_url.trim().trim_end_matches('/');
    let local_http = base.strip_prefix("http://").is_some_and(|rest| {
        let host = rest.split('/').next().unwrap_or_default();
        ["localhost", "127.0.0.1"]
            .into_iter()
            .any(|local| host == local || host.starts_with(&format!("{local}:")))
    });
    if !base.starts_with("https://") && !local_http {
        return Err("AI Base URL must use HTTPS, or local HTTP.".to_owned());
    }
    if config.model.trim().is_empty() || prompt.trim().is_empty() {
        return Err("Configure a model and enter an editing request.".to_owned());
    }
    let timeline = project.active();
    let audio = selected_audio(project, selected);
    let context = json!({
        "media": project.media.iter().map(|item| json!({"id":item.id,"name":item.name,"path":item.path,"duration":item.duration,"kind":item.kind,"hasAudio":item.has_audio})).collect::<Vec<_>>(),
        "timeline": {"id":timeline.id,"name":timeline.name,"video":timeline.video,"tracks":timeline.tracks,"clips":timeline.clips.iter().map(|clip| json!({"id":clip.id,"name":clip.name,"mediaId":clip.media_id,"trackId":clip.track_id,"start":clip.start,"duration":clip.duration,"sourceStart":clip.source_start,"speed":clip.speed,"volume":clip.volume,"kind":clip.kind,"text":clip.text.as_ref().map(|style| &style.content),"transitionIn":clip.transition_in,"fadeIn":clip.fade_in,"fadeOut":clip.fade_out})).collect::<Vec<_>>()},
        "audioEnergy": audio,
        "audioTranscript": transcript,
    });
    let message = format!(
        "Current project: {context}\nTrim rule: trimClip delta moves an edge to the right when positive. To shorten the tail use a negative delta; to shorten the head use a positive delta.\nUser request: {prompt}"
    );
    let mut text_body = json!({"model":config.model,"messages":[{"role":"system","content":SYSTEM},{"role":"user","content":message}]});
    let previews = preview_parts(project, selected);
    let mut body = if previews.is_empty() {
        text_body.clone()
    } else {
        let mut content = vec![json!({"type":"text","text":message})];
        content.extend(previews);
        json!({"model":config.model,"messages":[{"role":"system","content":SYSTEM},{"role":"user","content":content}]})
    };
    if let Some(effort) = config.reasoning_effort.as_deref() {
        if !["none", "low", "medium", "high", "xhigh", "max"].contains(&effort) {
            return Err("Invalid AI reasoning effort.".to_owned());
        }
        text_body["reasoning_effort"] = json!(effort);
        body["reasoning_effort"] = json!(effort);
    }
    let url = format!("{base}/chat/completions");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(90))
        .build();
    let post = |body: &Value| {
        let mut call = agent.post(&url).set("Content-Type", "application/json");
        if !config.api_key.is_empty() {
            call = call.set("Authorization", &format!("Bearer {}", config.api_key));
        }
        call.send_string(&body.to_string())
    };
    let post_retry = |body: &Value| match post(body) {
        Err(ureq::Error::Status(status, _)) if [502, 503, 504].contains(&status) => {
            std::thread::sleep(std::time::Duration::from_millis(500));
            post(body)
        }
        result => result,
    };
    let response = match post_retry(&body) {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _))
            if body != text_body && [400, 413, 415, 422].contains(&status) =>
        {
            post_retry(&text_body).map_err(|error| format!("AI request failed: {error}"))?
        }
        Err(error) => return Err(format!("AI request failed: {error}")),
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read AI response: {error}"))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("AI response is not JSON: {error}"))?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or("AI response contains no message content.")?;
    parse(content, project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_model_edits_remain_undoable() {
        let (Ok(base_url), Ok(model), Ok(api_key)) = (
            std::env::var("SHARBCUT_AGENT_TEST_BASE_URL"),
            std::env::var("SHARBCUT_AGENT_TEST_MODEL"),
            std::env::var("SHARBCUT_AGENT_TEST_API_KEY"),
        ) else {
            return;
        };
        let mut editor = concat_project::Editor::new();
        let plan = request(
            Config {
                base_url,
                model,
                api_key,
                reasoning_effort: Some("max".to_owned()),
            },
            "在当前空时间线的 0 秒位置添加一段持续 2 秒、内容为‘验收’的文字。只执行这一项编辑。",
            editor.project(),
            &[],
            None,
        )
        .expect("live model returns a safe edit");
        let command = plan.command.expect("the model proposed an edit");
        editor.apply(command).expect("edit applies");
        assert_eq!(editor.project().active().clips.len(), 1);
        assert!(editor.undo());
        assert!(editor.project().active().clips.is_empty());
        assert!(editor.redo());
        assert_eq!(editor.project().active().clips.len(), 1);
    }

    #[test]
    fn live_model_sees_visual_context() {
        let (Ok(base_url), Ok(model), Ok(api_key), Ok(path)) = (
            std::env::var("SHARBCUT_AGENT_TEST_BASE_URL"),
            std::env::var("SHARBCUT_AGENT_TEST_MODEL"),
            std::env::var("SHARBCUT_AGENT_TEST_API_KEY"),
            std::env::var("SHARBCUT_AGENT_TEST_IMAGE"),
        ) else {
            return;
        };
        let mut project = Project::new();
        project.media.push(
            serde_json::from_value(json!({
                "id":"visual1","path":path,"name":"visual sample",
                "kind":"image","hasAudio":false
            }))
            .expect("image media"),
        );
        let plan = request(
            Config {
                base_url,
                model,
                api_key,
                reasoning_effort: Some("max".to_owned()),
            },
            "只根据附带的画面预览，指出主要颜色。不要编辑时间线。",
            &project,
            &[],
            None,
        )
        .expect("live visual request");
        assert!(plan.reply.contains('红') || plan.reply.to_lowercase().contains("red"));
        assert!(plan.command.is_none());
    }

    #[test]
    fn live_model_routes_bgm_to_lite() {
        let (Ok(base_url), Ok(model), Ok(api_key), Ok(bgm), Ok(video)) = (
            std::env::var("SHARBCUT_AGENT_TEST_BASE_URL"),
            std::env::var("SHARBCUT_AGENT_TEST_MODEL"),
            std::env::var("SHARBCUT_AGENT_TEST_API_KEY"),
            std::env::var("SHARBCUT_AGENT_TEST_BGM"),
            std::env::var("SHARBCUT_AGENT_TEST_VIDEO"),
        ) else {
            return;
        };
        let mut project = Project::new();
        project.media.push(
            serde_json::from_value(json!({
                "id":"music1","path":bgm.clone(),"name":"project music",
                "kind":"audio","hasAudio":true
            }))
            .expect("BGM media"),
        );
        project.media.push(
            serde_json::from_value(json!({
                "id":"shot1","path":video.clone(),"name":"project footage",
                "kind":"video","hasAudio":false
            }))
            .expect("video media"),
        );
        let plan = request(
            Config {
                base_url,
                model,
                api_key,
                reasoning_effort: Some("max".to_owned()),
            },
            "用已导入的 BGM 和视频自动卡点成 5 秒，结果直接进入可编辑时间线。",
            &project,
            &[],
            None,
        )
        .expect("live montage request");
        let montage = plan.montage.expect("the model selected the montage path");
        assert_eq!(montage.duration, 5.0);
        assert_eq!(montage.bgm, PathBuf::from(bgm));
        assert_eq!(montage.videos, vec![PathBuf::from(video)]);
        assert!(plan.command.is_none());
    }

    #[test]
    fn live_model_reedits_bmts_timeline() {
        let (Ok(base_url), Ok(model), Ok(api_key), Ok(plan_path)) = (
            std::env::var("SHARBCUT_AGENT_TEST_BASE_URL"),
            std::env::var("SHARBCUT_AGENT_TEST_MODEL"),
            std::env::var("SHARBCUT_AGENT_TEST_API_KEY"),
            std::env::var("SHARBCUT_AGENT_TEST_PLAN"),
        ) else {
            return;
        };
        let mut editor = concat_project::Editor::new();
        let montage = crate::montage::prepare(std::path::Path::new(&plan_path), editor.project())
            .expect("real BMTS plan");
        editor.apply(montage.command).expect("import BMTS edit");
        let clip = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|clip| clip.kind == concat_project::model::ClipKind::Video)
            .expect("video cut")
            .clone();
        let edit = |editor: &mut concat_project::Editor, selected_id: &str, prompt: String| {
            let before = editor.project().clone();
            let plan = request(
                Config {
                    base_url: base_url.clone(),
                    model: model.clone(),
                    api_key: api_key.clone(),
                    reasoning_effort: Some("max".to_owned()),
                },
                &prompt,
                editor.project(),
                &[selected_id.to_owned()],
                None,
            )
            .unwrap_or_else(|error| panic!("{prompt}: {error}"));
            editor
                .apply(plan.command.expect("timeline edit"))
                .expect("apply");
            assert!(editor.undo());
            assert_eq!(editor.project(), &before);
            assert!(editor.redo());
        };
        let count = editor.project().active().clips.len();
        edit(
            &mut editor,
            &clip.id,
            format!(
                "只把片段 {} 在时间线 {:.3} 秒处分割，不做其他修改。",
                clip.id,
                clip.start + clip.duration * 0.5
            ),
        );
        assert_eq!(editor.project().active().clips.len(), count + 1);
        edit(
            &mut editor,
            &clip.id,
            format!("只把片段 {} 的速度改成 2 倍，不做其他修改。", clip.id),
        );
        assert_eq!(
            editor
                .project()
                .active()
                .clips
                .iter()
                .find(|item| item.id == clip.id)
                .unwrap()
                .speed,
            2.0
        );
        edit(
            &mut editor,
            &clip.id,
            format!("只把片段 {} 移到时间线 6 秒处，不做其他修改。", clip.id),
        );
        assert_eq!(
            editor
                .project()
                .active()
                .clips
                .iter()
                .find(|item| item.id == clip.id)
                .unwrap()
                .start,
            6.0
        );
        edit(
            &mut editor,
            &clip.id,
            format!("只删除片段 {}，不要删除素材库文件。", clip.id),
        );
        assert!(
            !editor
                .project()
                .active()
                .clips
                .iter()
                .any(|item| item.id == clip.id)
        );
        let next = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|item| item.kind == concat_project::model::ClipKind::Video)
            .expect("remaining video cut")
            .clone();
        edit(
            &mut editor,
            &next.id,
            format!("只将片段 {} 的尾部缩短 0.1 秒，不做其他修改。", next.id),
        );
        let trimmed = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|item| item.id == next.id)
            .unwrap();
        assert!(
            (trimmed.duration - (next.duration - 0.1)).abs() < 0.01,
            "trimmed from {} to {}",
            next.duration,
            trimmed.duration
        );
        edit(
            &mut editor,
            &next.id,
            format!(
                "只给片段 {} 添加 0.15 秒交叉淡化转场，不做其他修改。",
                next.id
            ),
        );
        let transitioned = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|item| item.id == next.id)
            .unwrap();
        assert_eq!(
            transitioned.transition_in.as_ref().unwrap().id,
            "cross-fade"
        );
        assert!((transitioned.transition_in.as_ref().unwrap().duration - 0.15).abs() < 0.001);
        let music = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|item| item.kind == concat_project::model::ClipKind::Audio)
            .expect("BGM cut")
            .id
            .clone();
        edit(
            &mut editor,
            &music,
            format!("只把背景音乐片段 {} 的音量调到 30%，不做其他修改。", music),
        );
        let bgm = editor
            .project()
            .active()
            .clips
            .iter()
            .find(|item| item.id == music)
            .unwrap();
        assert!((bgm.volume - 0.3).abs() < 0.001);
    }

    #[test]
    fn live_model_rough_cuts_real_media() {
        let (Ok(base_url), Ok(model), Ok(api_key), Ok(video_a), Ok(video_b)) = (
            std::env::var("SHARBCUT_AGENT_TEST_BASE_URL"),
            std::env::var("SHARBCUT_AGENT_TEST_MODEL"),
            std::env::var("SHARBCUT_AGENT_TEST_API_KEY"),
            std::env::var("SHARBCUT_AGENT_TEST_VIDEO"),
            std::env::var("SHARBCUT_AGENT_TEST_VIDEO_2"),
        ) else {
            return;
        };
        let mut editor = concat_project::Editor::new();
        for path in [&video_a, &video_b] {
            editor
                .apply(Command::AddMedia {
                    item: media::probe(path).expect("real video probe").to_new_media(),
                })
                .expect("import video");
        }
        let before = editor.project().clone();
        let plan = request(
            Config {
                base_url,
                model,
                api_key,
                reasoning_effort: Some("max".to_owned()),
            },
            "素材库有两条视频，时间线还是空的。各取至少一段，把它们粗剪成总长约 3 秒的可编辑时间线；不要生成最终视频文件，也不要自动卡点。",
            editor.project(),
            &[],
            None,
        )
        .expect("live rough cut");
        editor
            .apply(plan.command.expect("rough cut edits"))
            .expect("apply");
        let timeline = editor.project().active();
        assert!(timeline.clips.len() >= 2);
        let end = timeline
            .clips
            .iter()
            .map(|clip| clip.start + clip.duration)
            .fold(0.0_f64, f64::max);
        assert!((2.0..=4.0).contains(&end), "rough cut ends at {end}");
        assert!(
            timeline
                .clips
                .iter()
                .map(|clip| clip.media_id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len()
                >= 2
        );
        assert!(editor.undo());
        assert_eq!(editor.project(), &before);
        assert!(editor.redo());
        assert!(editor.project().active().clips.len() >= 2);
    }

    #[test]
    fn only_undoable_timeline_edits_cross_model_boundary() {
        let project = Project::new();
        let text = r#"{"reply":"已添加文字","commands":[{"op":"addTextClip","trackId":"T1","start":0,"duration":2,"style":{"content":"你好"}}]}"#;
        let plan = parse(text, &project).expect("safe plan");
        assert_eq!(plan.edits, 1);
        assert!(
            parse(
                r#"{"reply":"x","commands":[{"op":"removeMedia","mediaId":"m1"}]}"#,
                &project
            )
            .is_err()
        );
        assert!(
            parse(
                r#"{"reply":"x","commands":[{"op":"removeClips","clipIds":["missing"]}]}"#,
                &project
            )
            .is_err()
        );
    }

    #[test]
    fn montage_uses_only_imported_media_and_defaults_to_lite_inputs() {
        let mut project = Project::new();
        project.media.push(
            serde_json::from_value(json!({
                "id":"m1","path":"C:/music.mp3","name":"music","duration":60,
                "kind":"audio","hasAudio":true
            }))
            .expect("audio"),
        );
        project.media.push(
            serde_json::from_value(json!({
                "id":"m2","path":"D:/shot.mp4","name":"shot","duration":20,
                "kind":"video","hasAudio":false
            }))
            .expect("video"),
        );
        let response = r#"{"reply":"开始卡点","montage":{"bgmMediaId":"m1"},"commands":[]}"#;
        let plan = parse(response, &project).expect("checked montage");
        let montage = plan.montage.expect("montage action");
        assert_eq!(montage.duration, 30.0);
        assert_eq!(montage.bgm, PathBuf::from("C:/music.mp3"));
        assert_eq!(montage.videos, vec![PathBuf::from("D:/shot.mp4")]);
        assert!(plan.command.is_none());
        assert!(
            parse(
                r#"{"reply":"x","montage":{"bgmMediaId":"missing"}}"#,
                &project
            )
            .is_err()
        );
        assert!(
            parse(
                r#"{"reply":"x","montage":{"bgmMediaId":"m1","videoMediaIds":["m1"]}}"#,
                &project
            )
            .is_err()
        );
        assert!(
            parse(
                r#"{"reply":"x","montage":{"bgmMediaId":"m1"},"commands":[{"op":"addTextClip","start":0,"duration":1,"style":{"content":"x"}}]}"#,
                &project
            )
            .is_err()
        );
        for index in 3..=27 {
            let mut video = project.media[1].clone();
            video.id = format!("m{index}");
            video.path = format!("D:/shot-{index}.mp4");
            project.media.push(video);
        }
        assert_eq!(
            parse(response, &project)
                .unwrap()
                .montage
                .unwrap()
                .videos
                .len(),
            24
        );
    }

    #[test]
    fn real_video_preview_is_labeled_and_bounded() {
        use std::io::{BufRead, BufReader, Read, Write};

        let Ok(path) = std::env::var("SHARBCUT_AGENT_TEST_VIDEO") else {
            return;
        };
        let mut project = Project::new();
        project.media.push(
            serde_json::from_value(json!({
                "id":"m1","path":path,"name":"test video","duration":12,
                "kind":"video","hasAudio":false
            }))
            .expect("video"),
        );
        let parts = preview_parts(&project, &[]);
        assert_eq!(parts.len(), 2);
        assert!(parts[0]["text"].as_str().unwrap().contains("media m1"));
        assert!(
            parts[1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,/9j/")
        );
        let server = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let address = server.local_addr().expect("address");
        let worker = std::thread::spawn(move || {
            for (index, image_expected) in [true, true, false].into_iter().enumerate() {
                let (stream, _) = server.accept().expect("request");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).expect("request line");
                let mut length = 0;
                loop {
                    line.clear();
                    stream.read_line(&mut line).expect("header");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().expect("length");
                    }
                }
                let mut request = vec![0; length];
                stream.read_exact(&mut request).expect("body");
                let request: Value = serde_json::from_slice(&request).expect("json body");
                assert_eq!(request["messages"][1]["content"].is_array(), image_expected);
                let (status, body) = if index == 0 {
                    ("502 Bad Gateway", "{}".to_owned())
                } else if image_expected {
                    ("400 Bad Request", "{}".to_owned())
                } else {
                    ("200 OK", json!({"choices":[{"message":{"content":r#"{"reply":"fallback","commands":[]}"#}}]}).to_string())
                };
                write!(stream.get_mut(), "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
            }
        });
        let plan = request(
            Config {
                base_url: format!("http://{address}/v1"),
                model: "mock".into(),
                api_key: String::new(),
                reasoning_effort: None,
            },
            "描述画面",
            &project,
            &[],
            None,
        )
        .expect("text fallback");
        worker.join().expect("server");
        assert_eq!(plan.reply, "fallback");
    }

    #[test]
    fn real_audio_energy_is_bounded() {
        let Ok(path) = std::env::var("SHARBCUT_AGENT_TEST_AUDIO") else {
            return;
        };
        let levels = audio_levels(&path, 0.0, 5.0, None).expect("audio levels");
        assert!(!levels.is_empty() && levels.len() <= 3);
        assert!(
            levels
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
        );
    }

    #[test]
    fn openai_compatible_request_returns_checked_edit() {
        use std::io::{BufRead, BufReader, Read, Write};

        let server = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback");
        let address = server.local_addr().expect("address");
        let worker = std::thread::spawn(move || {
            let (stream, _) = server.accept().expect("request");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).expect("request line");
            assert!(line.contains("POST /v1/chat/completions"));
            let mut length = 0;
            loop {
                line.clear();
                stream.read_line(&mut line).expect("header");
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().expect("content length");
                }
            }
            assert!(length > 0, "request has a body");
            let mut request = vec![0; length];
            stream.read_exact(&mut request).expect("request body");
            let request: Value = serde_json::from_slice(&request).expect("request json");
            assert_eq!(request["reasoning_effort"], "max");
            assert!(request.get("temperature").is_none());
            assert!(
                request["messages"][1]["content"]
                    .as_str()
                    .unwrap()
                    .contains("selected speech")
            );
            let body = json!({"choices":[{"message":{"content":r#"{"reply":"已添加","commands":[{"op":"addTextClip","trackId":"T1","start":0,"duration":2,"style":{"content":"测试"}}]}"#}}]}).to_string();
            write!(stream.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        });
        let plan = request(
            Config {
                base_url: format!("http://{address}/v1"),
                model: "mock".to_owned(),
                api_key: "test-only".to_owned(),
                reasoning_effort: Some("max".to_owned()),
            },
            "添加文字",
            &Project::new(),
            &[],
            Some("selected speech"),
        )
        .expect("model edit");
        worker.join().expect("server");
        assert_eq!(plan.edits, 1);
    }
}
