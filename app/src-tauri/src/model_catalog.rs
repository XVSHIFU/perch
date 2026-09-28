use crate::store::{Failure, ModelSpec, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    io::Read,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogInput {
    pub connection_id: Option<String>,
    pub base_url: String,
    pub protocol: String,
    pub api_key: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogResult {
    pub models: Vec<ModelSpec>,
    pub truncated: bool,
}
pub fn validate_base_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value.trim())
        .map_err(|_| Failure::new("INVALID_URL", "API 地址不是有效网址"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https"
            || (url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
    {
        return Err(Failure::new(
            "INVALID_URL",
            "API 地址需使用 HTTPS（本机可 HTTP），不能包含凭据、查询参数或片段",
        ));
    }
    Ok(url)
}
pub fn endpoint(base: &str, protocol: &str) -> Result<url::Url> {
    if !["deepseek", "openai-chat", "openai-responses", "anthropic"].contains(&protocol) {
        return Err(Failure::new("INVALID_PROTOCOL", "接口协议不受支持"));
    }
    let mut url = validate_base_url(base)?;
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/models") {
        path.to_string()
    } else if path.is_empty() && protocol != "deepseek" {
        "/v1/models".into()
    } else {
        format!("{path}/models")
    };
    url.set_path(&path);
    Ok(url)
}
fn parse_page(value: &serde_json::Value) -> Result<Vec<ModelSpec>> {
    let data = value
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            Failure::new("MODEL_FORMAT", "服务端未返回有效模型列表，可以手动添加模型")
        })?;
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for row in data.iter().take(2000) {
        let Some(id) = row.get("id").and_then(|v| v.as_str()).map(str::trim) else {
            continue;
        };
        if id.is_empty()
            || id.len() > 512
            || id.chars().any(char::is_control)
            || !seen.insert(id.to_string())
        {
            continue;
        }
        let name = row
            .get("display_name")
            .or_else(|| row.get("name"))
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
            .unwrap_or(id);
        models.push(ModelSpec {
            id: id.into(),
            name: name.into(),
            context_window: None,
            max_tokens: None,
        });
    }
    Ok(models)
}
pub fn fetch(base: &str, protocol: &str, key: &str) -> Result<CatalogResult> {
    if key.is_empty() || key.len() > 2400 || key.chars().any(char::is_control) {
        return Err(Failure::new(
            "KEY_REQUIRED",
            "请填写有效 API Key，或使用已保存的连接",
        ));
    }
    let mut url = endpoint(base, protocol)?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| Failure::new("NETWORK_INIT", "网络服务初始化失败"))?;
    let started = Instant::now();
    let mut models = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    for _ in 0..10 {
        let remaining = Duration::from_secs(25)
            .checked_sub(started.elapsed())
            .ok_or_else(|| {
                Failure::new("MODEL_TIMEOUT", "获取模型超时，原模型目录未变，请稍后重试")
            })?;
        let request = client
            .get(url.clone())
            .timeout(remaining)
            .header("Accept", "application/json");
        let request = if protocol == "anthropic" {
            request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
        } else {
            request.bearer_auth(key)
        };
        let response = request.send().map_err(|e| {
            if e.is_timeout() {
                Failure::new("MODEL_TIMEOUT", "获取模型超时，请检查网络或稍后重试")
            } else {
                Failure::new(
                    "MODEL_NETWORK",
                    "无法连接模型服务，请检查 API 地址、代理和网络",
                )
            }
        })?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => Failure::new("MODEL_AUTH", "服务商拒绝了认证，请检查 Key 和账户权限"),
                404 | 405 => Failure::new(
                    "MODEL_UNSUPPORTED",
                    "此地址未提供模型列表接口；请确认 API 基础地址，或手动添加模型",
                ),
                429 => Failure::new("MODEL_RATE_LIMIT", "请求过于频繁，请稍后重试"),
                300..=399 => Failure::new(
                    "MODEL_REDIRECT",
                    "服务返回了重定向；为避免 Key 被转发，请直接填写最终 API 地址",
                ),
                _ => Failure::new(
                    "MODEL_SERVER",
                    "模型服务暂时不可用，原模型目录未变，请稍后重试",
                ),
            });
        }
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| Failure::new("MODEL_READ", "模型列表读取失败，请重试"))?;
        if bytes.len() > 1_048_576 {
            return Err(Failure::new(
                "MODEL_SIZE",
                "返回的模型列表过大，请手动添加需要的模型",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
            Failure::new(
                "MODEL_FORMAT",
                "服务端没有返回模型 JSON 列表，请检查 API 地址",
            )
        })?;
        for model in parse_page(&value)? {
            if ids.insert(model.id.clone()) {
                models.push(model);
            }
        }
        if models.len() > 1000 {
            models.truncate(1000);
            return Ok(CatalogResult {
                models,
                truncated: true,
            });
        }
        if value
            .get("data")
            .and_then(|v| v.as_array())
            .is_some_and(|v| v.len() > 2000)
        {
            return Ok(CatalogResult {
                models,
                truncated: true,
            });
        }
        if value.get("has_more").and_then(|v| v.as_bool()) != Some(true) {
            return Ok(CatalogResult {
                models,
                truncated: false,
            });
        }
        let cursor = value
            .get("last_id")
            .and_then(|v| v.as_str())
            .or_else(|| models.last().map(|m| m.id.as_str()))
            .unwrap_or("")
            .to_string();
        if protocol != "anthropic" || cursor.is_empty() || !cursors.insert(cursor.clone()) {
            return Ok(CatalogResult {
                models,
                truncated: true,
            });
        }
        url.query_pairs_mut()
            .clear()
            .append_pair("after_id", &cursor)
            .append_pair("limit", "1000");
    }
    Ok(CatalogResult {
        models,
        truncated: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    fn serve(status: &str, headers: &str, body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/custom/v3", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let h = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = [0; 8192];
            let n = stream.read(&mut bytes).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&bytes[..n]).into_owned()
        });
        (url, h)
    }
    #[test]
    fn catalog_success_and_failure_boundaries() {
        let (url, h) = serve(
            "200 OK",
            "Content-Type: application/json\r\n",
            r#"{"data":[{"id":"test-model","display_name":"测试模型"},{"id":"test-model"}]}"#,
        );
        let result = fetch(&url, "openai-chat", "test-only-key").unwrap();
        assert_eq!(result.models.len(), 1);
        assert_eq!(result.models[0].name, "测试模型");
        let request = h.join().unwrap();
        assert!(request.starts_with("GET /custom/v3/models "));
        assert!(request
            .to_lowercase()
            .contains("authorization: bearer test-only-key"));
        let (url, h) = serve("401 Unauthorized", "", "test-only-key");
        let error = fetch(&url, "deepseek", "test-only-key").err().unwrap();
        assert_eq!(error.code, "MODEL_AUTH");
        assert!(!error.message.contains("test-only-key"));
        h.join().unwrap();
        let (url, h) = serve("302 Found", "Location: https://example.invalid/\r\n", "");
        assert_eq!(
            fetch(&url, "deepseek", "test-only-key").err().unwrap().code,
            "MODEL_REDIRECT"
        );
        h.join().unwrap();
        assert_eq!(
            endpoint("https://example.com", "anthropic").unwrap().path(),
            "/v1/models"
        );
        assert_eq!(
            endpoint("https://example.com/api/v3/", "openai-chat")
                .unwrap()
                .path(),
            "/api/v3/models"
        );
        assert_eq!(
            endpoint("https://api.deepseek.com", "deepseek")
                .unwrap()
                .path(),
            "/models"
        );
    }
}
