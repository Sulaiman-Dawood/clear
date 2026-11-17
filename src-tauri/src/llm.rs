//! Talking to LLM providers over HTTP with streaming.
//!
//! Two wire formats cover every supported provider:
//! - Anthropic's Messages API (Claude), and
//! - the OpenAI-compatible Chat Completions API (OpenAI, Groq, OpenRouter,
//!   Mistral, DeepSeek, Gemini, Ollama, LM Studio, and custom servers).

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Wire {
    Anthropic,
    OpenAi,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub wire: Wire,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub needs_key: bool,
    pub local: bool,
    /// Model ids offered as suggestions before "Load models" is used.
    pub suggested_models: &'static [&'static str],
    pub key_hint: &'static str,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "anthropic",
        name: "Anthropic (Claude)",
        wire: Wire::Anthropic,
        base_url: "https://api.anthropic.com",
        default_model: "claude-opus-5-5",
        needs_key: true,
        local: false,
        suggested_models: &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
        key_hint: "Create a key at platform.claude.com",
    },
    ProviderPreset {
        id: "openai",
        name: "OpenAI",
        wire: Wire::OpenAi,
        base_url: "https://api.openai.com/v1",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key at platform.openai.com",
    },
    ProviderPreset {
        id: "gemini",
        name: "Google Gemini",
        wire: Wire::OpenAi,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key in Google AI Studio",
    },
    ProviderPreset {
        id: "groq",
        name: "Groq",
        wire: Wire::OpenAi,
        base_url: "https://api.groq.com/openai/v1",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key at console.groq.com",
    },
    ProviderPreset {
        id: "openrouter",
        name: "OpenRouter",
        wire: Wire::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key at openrouter.ai",
    },
    ProviderPreset {
        id: "mistral",
        name: "Mistral",
        wire: Wire::OpenAi,
        base_url: "https://api.mistral.ai/v1",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key at console.mistral.ai",
    },
    ProviderPreset {
        id: "deepseek",
        name: "DeepSeek",
        wire: Wire::OpenAi,
        base_url: "https://api.deepseek.com/v1",
        default_model: "",
        needs_key: true,
        local: false,
        suggested_models: &[],
        key_hint: "Create a key at platform.deepseek.com",
    },
    ProviderPreset {
        id: "ollama",
        name: "Ollama (local)",
        wire: Wire::OpenAi,
        base_url: "http://localhost:11434/v1",
        default_model: "",
        needs_key: false,
        local: true,
        suggested_models: &[],
        key_hint: "Runs on this computer. Start Ollama, then load models.",
    },
    ProviderPreset {
        id: "lmstudio",
        name: "LM Studio (local)",
        wire: Wire::OpenAi,
        base_url: "http://localhost:1234/v1",
        default_model: "",
        needs_key: false,
        local: true,
        suggested_models: &[],
        key_hint: "Runs on this computer. Start LM Studio's server, then load models.",
    },
    ProviderPreset {
        id: "custom",
        name: "Custom (OpenAI-compatible)",
        wire: Wire::OpenAi,
        base_url: "",
        default_model: "",
        needs_key: false,
        local: false,
        suggested_models: &[],
        key_hint: "Any server that speaks the OpenAI Chat Completions API. The key is optional.",
    },
];

pub fn preset(id: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|p| p.id == id)
}

/// Everything needed to make one request.
#[derive(Clone)]
pub struct Target {
    pub provider: String,
    pub wire: Wire,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub effort: String,
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(90))
        .build()
        .unwrap_or_default()
}

/// Effort is accepted by Claude Opus 4.5+ and the current generation;
/// older models reject it.
fn supports_effort(model: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "claude-opus-5",
        "claude-sonnet-5",
        "claude-haiku-5",
        "claude-fable",
        "claude-mythos",
        "claude-opus-4-5",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-sonnet-4-6",
    ];
    PREFIXES.iter().any(|p| model.starts_with(p))
}

/// Models where the server-side refusal fallback (`fallbacks: "default"`) is available.
fn supports_fallbacks(model: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-fable-5-1",
        "claude-fable-5-2",
        "claude-mythos-5-1",
        "claude-mythos-5-2",
    ];
    PREFIXES.iter().any(|p| model.starts_with(p))
}

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn validate(t: &Target) -> Result<(), String> {
    if t.base_url.trim().is_empty() {
        return Err("Set the server URL in Settings > AI provider.".into());
    }
    if t.model.trim().is_empty() {
        return Err("Choose a model in Settings > AI provider.".into());
    }
    let needs_key = preset(&t.provider).map(|p| p.needs_key).unwrap_or(false);
    if needs_key && t.api_key.is_none() {
        return Err("Add your API key in Settings > AI provider.".into());
    }
    Ok(())
}

fn net_error(e: reqwest::Error, base: &str) -> String {
    if e.is_timeout() {
        "The request timed out. Try again, or pick a faster model.".into()
    } else if e.is_connect() {
        format!("Can't reach {base}. Check your connection, or that the local server is running.")
    } else {
        format!("Network error: {e}")
    }
}

fn api_error(status: reqwest::StatusCode, body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    let hint = match status.as_u16() {
        401 | 403 => " Check your API key.",
        404 => " Check the model name and server URL.",
        429 => " You've hit a rate limit; wait a moment and try again.",
        529 | 503 => " The provider is overloaded; try again shortly.",
        _ => "",
    };
    format!("{} {}{}", status.as_u16(), message.trim(), hint)
}

/// Stream a completion, calling `on_delta` with each new piece of text.
/// Returns the full text.
pub async fn stream(
    client: &reqwest::Client,
    t: &Target,
    system: &str,
    user: &str,
    mut on_delta: impl FnMut(&str),
) -> Result<String, String> {
    validate(t)?;

    let request = match t.wire {
        Wire::Anthropic => {
            let mut body = json!({
                "model": t.model,
                "max_tokens": 32000,
                "stream": true,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            });
            if supports_effort(&t.model) {
                body["output_config"] = json!({ "effort": t.effort });
            }
            let mut req = client
                .post(join(&t.base_url, "v1/messages"))
                .header("x-api-key", t.api_key.clone().unwrap_or_default())
                .header("anthropic-version", "2026-10-21");
            if supports_fallbacks(&t.model) {
                // If a safety classifier declines, the API retries on a
                // suitable fallback model inside the same request.
                body["fallbacks"] = json!("default");
                req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
            }
            req.json(&body)
        }
        Wire::OpenAi => {
            let body = json!({
                "model": t.model,
                "stream": true,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            });
            let mut req = client.post(join(&t.base_url, "chat/completions")).json(&body);
            if let Some(key) = &t.api_key {
                req = req.bearer_auth(key);
            }
            req
        }
    };

    let resp = request.send().await.map_err(|e| net_error(e, &t.base_url))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(api_error(status, &body));
    }

    let mut bytes = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut out = String::new();
    let mut stop_reason: Option<String> = None;

    'read: while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|e| net_error(e, &t.base_url))?;
        buf.extend_from_slice(&chunk);
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim_start();
            if data == "[DONE]" {
                break 'read;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            match t.wire {
                Wire::Anthropic => match v["type"].as_str().unwrap_or_default() {
                    "content_block_delta" => {
                        if v["delta"]["type"] == "text_delta" {
                            if let Some(text) = v["delta"]["text"].as_str() {
                                out.push_str(text);
                                on_delta(text);
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(r) = v["delta"]["stop_reason"].as_str() {
                            stop_reason = Some(r.to_string());
                        }
                    }
                    "message_stop" => break 'read,
                    "error" => {
                        let msg = v["error"]["message"].as_str().unwrap_or("Unknown error");
                        return Err(format!("The provider reported an error: {msg}"));
                    }
                    _ => {}
                },
                Wire::OpenAi => {
                    if let Some(msg) = v.pointer("/error/message").and_then(Value::as_str) {
                        return Err(format!("The provider reported an error: {msg}"));
                    }
                    let choice = &v["choices"][0];
                    if let Some(text) = choice["delta"]["content"].as_str() {
                        if !text.is_empty() {
                            out.push_str(text);
                            on_delta(text);
                        }
                    }
                    if let Some(r) = choice["finish_reason"].as_str() {
                        stop_reason = Some(r.to_string());
                    }
                }
            }
        }
    }

    match stop_reason.as_deref() {
        Some("refusal") | Some("content_filter") => {
            return Err("The model declined this request. Try different wording or another model.".into())
        }
        _ => {}
    }
    if out.trim().is_empty() {
        return Err("The model returned an empty response.".into());
    }
    Ok(out)
}

/// List the model ids a provider offers.
pub async fn list_models(client: &reqwest::Client, t: &Target) -> Result<Vec<String>, String> {
    if t.base_url.trim().is_empty() {
        return Err("Set the server URL first.".into());
    }
    let req = match t.wire {
        Wire::Anthropic => client
            .get(join(&t.base_url, "v1/models?limit=100"))
            .header("x-api-key", t.api_key.clone().unwrap_or_default())
            .header("anthropic-version", "2026-10-21"),
        Wire::OpenAi => {
            let mut r = client.get(join(&t.base_url, "models"));
            if let Some(key) = &t.api_key {
                r = r.bearer_auth(key);
            }
            r
        }
    };
    let resp = req.send().await.map_err(|e| net_error(e, &t.base_url))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api_error(status, &body));
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "Unexpected response from the server.".to_string())?;
    let list = v["data"]
        .as_array()
        .or_else(|| v["models"].as_array())
        .ok_or("The server didn't return a model list.")?;
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
        .map(|s| s.trim_start_matches("models/").to_string())
        .collect();
    if t.wire == Wire::OpenAi {
        ids.sort();
    }
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serve one HTTP response, writing the body in small chunks so the
    /// parser has to handle lines split across reads.
    fn serve_once(status: &str, content_type: &str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status.to_string();
        let content_type = content_type.to_string();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf);
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).unwrap();
            for chunk in body.as_bytes().chunks(7) {
                sock.write_all(chunk).unwrap();
                sock.flush().unwrap();
            }
        });
        format!("http://{addr}")
    }

    fn target(wire: Wire, base_url: String) -> Target {
        Target {
            provider: "custom".into(),
            wire,
            base_url,
            model: "test-model".into(),
            api_key: Some("k".into()),
            effort: "low".into(),
        }
    }

    fn run(t: Target) -> (Result<String, String>, String) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut streamed = String::new();
        let result = rt.block_on(stream(&http_client(), &t, "sys", "user", |d| streamed.push_str(d)));
        (result, streamed)
    }

    #[test]
    fn parses_anthropic_stream() {
        let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{}}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Héllo, \"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"wörld ✓\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let url = serve_once("200 OK", "text/event-stream", body);
        let (result, streamed) = run(target(Wire::Anthropic, url));
        assert_eq!(result.unwrap(), "Héllo, wörld ✓");
        assert_eq!(streamed, "Héllo, wörld ✓");
    }

    #[test]
    fn parses_openai_stream() {
        let body = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\r\n\r\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Fixed \"}}]}\r\n\r\n\
data: {\"choices\":[{\"delta\":{\"content\":\"text.\"},\"finish_reason\":\"stop\"}]}\r\n\r\n\
data: [DONE]\r\n\r\n";
        let url = serve_once("200 OK", "text/event-stream", body);
        let (result, _) = run(target(Wire::OpenAi, url));
        assert_eq!(result.unwrap(), "Fixed text.");
    }

    #[test]
    fn reports_refusal() {
        let body = "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"}}\n\n\
data: {\"type\":\"message_stop\"}\n\n";
        let url = serve_once("200 OK", "text/event-stream", body);
        let (result, _) = run(target(Wire::Anthropic, url));
        assert!(result.unwrap_err().contains("declined"));
    }

    #[test]
    fn reports_http_errors_with_hint() {
        let body = "{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}";
        let url = serve_once("401 Unauthorized", "application/json", body);
        let (result, _) = run(target(Wire::Anthropic, url));
        let err = result.unwrap_err();
        assert!(err.contains("401") && err.contains("invalid x-api-key") && err.contains("API key"), "{err}");
    }

    #[test]
    fn requires_key_and_model() {
        let mut t = target(Wire::Anthropic, "http://127.0.0.1:9".into());
        t.provider = "anthropic".into();
        t.api_key = None;
        assert!(run(t.clone()).0.unwrap_err().contains("API key"));
        t.api_key = Some("k".into());
        t.model.clear();
        assert!(run(t).0.unwrap_err().contains("model"));
    }
}
