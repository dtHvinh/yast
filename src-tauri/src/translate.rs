use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub source_lang: String,
    pub target_lang: String,
    pub provider: String,
    #[serde(default)]
    pub deepl_key: String,
    #[serde(default)]
    pub libre_url: String,
    #[serde(default)]
    pub libre_key: String,
    pub interval_ms: u64,
    /// Fractions of the window: [x, y, width, height].
    pub region: [f32; 4],
}

pub struct Translator {
    client: reqwest::Client,
    cache: Mutex<HashMap<String, String>>,
}

impl Translator {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new(), cache: Mutex::new(HashMap::new()) }
    }

    pub async fn translate(&self, cfg: &Config, text: &str) -> Result<String, String> {
        let key = format!("{}|{}|{}|{}", cfg.provider, cfg.source_lang, cfg.target_lang, text);
        if let Some(hit) = self.cache.lock().unwrap().get(&key) {
            return Ok(hit.clone());
        }

        let out = match cfg.provider.as_str() {
            "deepl" => self.deepl(cfg, text).await?,
            "libre" => self.libre(cfg, text).await?,
            "mymemory" => self.mymemory(cfg, text).await?,
            _ => self.google(cfg, text).await?,
        };

        let mut cache = self.cache.lock().unwrap();
        if cache.len() > 5000 {
            cache.clear();
        }
        cache.insert(key, out.clone());
        Ok(out)
    }

    async fn google(&self, cfg: &Config, text: &str) -> Result<String, String> {
        let sl = if cfg.source_lang == "auto" { "auto".to_string() } else { google_code(&cfg.source_lang) };
        let response = self
            .client
            .get("https://translate.googleapis.com/translate_a/single")
            .query(&[("client", "gtx"), ("sl", &sl), ("tl", &cfg.target_lang), ("dt", "t"), ("q", text)])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err("Google's free endpoint is rate limiting this connection (HTTP 429). Pick another translation service.".into());
        }
        let resp: Value = response
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let segments = resp.get(0).and_then(Value::as_array).ok_or("unexpected Google response")?;
        Ok(segments.iter().filter_map(|s| s.get(0).and_then(Value::as_str)).collect())
    }

    /// Keyless, but limited to 500 bytes per request, so long text is sent in chunks.
    async fn mymemory(&self, cfg: &Config, text: &str) -> Result<String, String> {
        let pair = format!(
            "{}|{}",
            if cfg.source_lang == "auto" { "autodetect".to_string() } else { google_code(&cfg.source_lang) },
            cfg.target_lang
        );
        let mut out = Vec::new();
        for chunk in split_chunks(text, 450) {
            let resp: Value = self
                .client
                .get("https://api.mymemory.translated.net/get")
                .query(&[("q", chunk.as_str()), ("langpair", pair.as_str())])
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            let status = &resp["responseStatus"];
            if status.as_i64() != Some(200) && status.as_str() != Some("200") {
                return Err(format!("MyMemory: {}", resp["responseDetails"].as_str().unwrap_or("request failed")));
            }
            out.push(resp["responseData"]["translatedText"].as_str().ok_or("unexpected MyMemory response")?.to_owned());
        }
        Ok(out.join(" "))
    }

    async fn deepl(&self, cfg: &Config, text: &str) -> Result<String, String> {
        if cfg.deepl_key.is_empty() {
            return Err("DeepL API key is missing".into());
        }
        let host = if cfg.deepl_key.ends_with(":fx") { "api-free.deepl.com" } else { "api.deepl.com" };
        let mut body = json!({ "text": [text], "target_lang": deepl_target(&cfg.target_lang) });
        if cfg.source_lang != "auto" {
            body["source_lang"] = json!(base_lang(&cfg.source_lang).to_uppercase());
        }
        let resp: Value = self
            .client
            .post(format!("https://{host}/v2/translate"))
            .header("Authorization", format!("DeepL-Auth-Key {}", cfg.deepl_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        resp["translations"][0]["text"].as_str().map(str::to_owned).ok_or("unexpected DeepL response".into())
    }

    /// LibreTranslate-compatible server; self-host one to translate fully offline.
    async fn libre(&self, cfg: &Config, text: &str) -> Result<String, String> {
        if cfg.libre_url.is_empty() {
            return Err("LibreTranslate URL is missing".into());
        }
        let source = if cfg.source_lang == "auto" { "auto".to_string() } else { base_lang(&cfg.source_lang) };
        let mut body = json!({
            "q": text, "source": source, "target": base_lang(&cfg.target_lang), "format": "text"
        });
        if !cfg.libre_key.is_empty() {
            body["api_key"] = json!(cfg.libre_key);
        }
        let resp: Value = self
            .client
            .post(format!("{}/translate", cfg.libre_url.trim_end_matches('/')))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        resp["translatedText"].as_str().map(str::to_owned).ok_or("unexpected LibreTranslate response".into())
    }
}

/// Splits text into pieces of at most `max` bytes, preferring to break after whitespace.
fn split_chunks(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(space) = rest[..cut].rfind(char::is_whitespace) {
            cut = space + 1;
        }
        chunks.push(rest[..cut].trim().to_owned());
        rest = &rest[cut..];
    }
    if !rest.trim().is_empty() {
        chunks.push(rest.trim().to_owned());
    }
    chunks
}

/// "ja-JP" -> "ja"
fn base_lang(tag: &str) -> String {
    tag.split('-').next().unwrap_or(tag).to_ascii_lowercase()
}

fn google_code(tag: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    if lower.starts_with("zh") {
        if lower.contains("hant") || lower.ends_with("-tw") || lower.ends_with("-hk") { "zh-TW".into() } else { "zh-CN".into() }
    } else {
        base_lang(tag)
    }
}

fn deepl_target(tag: &str) -> String {
    match tag {
        "en" => "EN-US".into(),
        "pt" => "PT-BR".into(),
        "zh-CN" => "ZH-HANS".into(),
        "zh-TW" => "ZH-HANT".into(),
        other => other.to_ascii_uppercase(),
    }
}
