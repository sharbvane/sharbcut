// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 SharbCut contributors

//! A model proposes ordinary project commands; only a small edit allowlist
//! crosses back into the timeline. The UI applies the returned Batch once.

use std::io::Read;

use concat_project::commands::{ClipPatch, Command, IdMint, apply};
use concat_project::model::{Project, TextStyle};
use serde::Deserialize;
use serde_json::{Value, json};

/// User-selected OpenAI-compatible endpoint and credential.
pub struct Config {
    /// Endpoint prefix, usually ending in `/v1`.
    pub base_url: String,
    /// Model identifier understood by that endpoint.
    pub model: String,
    /// Per-user secret, never stored in the project.
    pub api_key: String,
}

/// A checked model proposal, ready to apply as one undo step.
pub struct Plan {
    /// Text shown to the user.
    pub reply: String,
    /// None when the model answered without making an edit.
    pub command: Option<Command>,
    /// Number of contained operations.
    pub edits: usize,
}

#[derive(Deserialize)]
struct ModelReply {
    reply: String,
    #[serde(default)]
    commands: Vec<Value>,
}

const SYSTEM: &str = r#"You are SharbCut's timeline editing assistant. Reply with ONE JSON object only: {"reply":"short Chinese explanation","commands":[...]}. Commands are SharbCut's existing camelCase command format. Allowed ops: removeClips {clipIds}, splitClips {clipIds,time}, trimClip {clipId,edge:"start"|"end",delta}, moveClips {moves:[{clipId,start,trackId}]}, setClipSpeed {clipId,speed}, updateClip {clipId,patch:{volume?,fadeIn?,fadeOut?,transitionIn?}}, addClipSegment {mediaPath,trackId?,start,sourceStart,duration,speed,patch:{}}, addTextClip {trackId?,start,duration,style:{content}}. Use ONLY ids and media paths from the provided project. Times are seconds. To make a rough cut, remove existing clips and add source-backed segments in one response. To place a BGM already in the bin, use addClipSegment on its path, with an audio track. If the request is ambiguous or needs content analysis unavailable in the context, explain the limitation and return no commands. Never invent source files or claim to have seen/heard footage. No markdown fences."#;

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
    let mut commands = Vec::with_capacity(response.commands.len());
    for value in response.commands {
        let command: Command = serde_json::from_value(value)
            .map_err(|error| format!("AI edit command is invalid: {error}"))?;
        validate(&command, project)?;
        commands.push(command);
    }
    let edits = commands.len();
    let command = if edits == 0 {
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
        edits,
    })
}

/// Ask the configured model to edit the current timeline off the UI thread.
pub fn request(config: Config, prompt: &str, project: &Project) -> Result<Plan, String> {
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
    let context = json!({
        "media": project.media.iter().map(|item| json!({"id":item.id,"name":item.name,"path":item.path,"duration":item.duration,"kind":item.kind,"hasAudio":item.has_audio})).collect::<Vec<_>>(),
        "timeline": {"id":timeline.id,"name":timeline.name,"video":timeline.video,"tracks":timeline.tracks,"clips":timeline.clips.iter().map(|clip| json!({"id":clip.id,"name":clip.name,"mediaId":clip.media_id,"trackId":clip.track_id,"start":clip.start,"duration":clip.duration,"sourceStart":clip.source_start,"speed":clip.speed,"volume":clip.volume,"kind":clip.kind})).collect::<Vec<_>>()},
    });
    let body = json!({"model":config.model,"temperature":0.2,"messages":[{"role":"system","content":SYSTEM},{"role":"user","content":format!("Current project: {context}\nUser request: {prompt}")}]});
    let url = format!("{base}/chat/completions");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(90))
        .build();
    let mut call = agent.post(&url).set("Content-Type", "application/json");
    if !config.api_key.is_empty() {
        call = call.set("Authorization", &format!("Bearer {}", config.api_key));
    }
    let response = call
        .send_string(&body.to_string())
        .map_err(|error| format!("AI request failed: {error}"))?;
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
            let body = json!({"choices":[{"message":{"content":r#"{"reply":"已添加","commands":[{"op":"addTextClip","trackId":"T1","start":0,"duration":2,"style":{"content":"测试"}}]}"#}}]}).to_string();
            write!(stream.get_mut(), "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("response");
        });
        let plan = request(
            Config {
                base_url: format!("http://{address}/v1"),
                model: "mock".to_owned(),
                api_key: "test-only".to_owned(),
            },
            "添加文字",
            &Project::new(),
        )
        .expect("model edit");
        worker.join().expect("server");
        assert_eq!(plan.edits, 1);
    }
}
