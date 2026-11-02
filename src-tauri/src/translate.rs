//! 翻译：腾讯云机器翻译 / 百度翻译开放平台 / 微软 Azure Translator（均使用官方免费额度）
use std::collections::HashMap;

use anyhow::{anyhow, bail, Context};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn is_mostly_chinese(texts: &[String]) -> bool {
    let (mut han, mut letters) = (0usize, 0usize);
    for c in texts.iter().flat_map(|t| t.chars()) {
        if ('\u{4e00}'..='\u{9fff}').contains(&c) {
            han += 1;
        } else if c.is_alphabetic() {
            letters += 1;
        }
    }
    han > 0 && han * 2 >= letters
}

fn need<'a>(secret: &'a HashMap<String, String>, k: &str, name: &str) -> anyhow::Result<&'a str> {
    secret
        .get(k)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("请先在设置中填写 {name}"))
}

/// 逐行翻译，返回与输入一一对应的结果。to 取值 zh / en
pub async fn translate(
    provider: &str,
    secret: &HashMap<String, String>,
    cfg: &crate::settings::Settings,
    texts: &[String],
    to: &str,
) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Screenshot/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(if provider == "openai" { 120 } else { 20 }))
        .build()?;
    let mut out = Vec::with_capacity(texts.len());
    // 按服务商的单次上限分批
    let limit = match provider {
        "baidu" => 5000,
        "azure" => 40000,
        "openai" => 3000,
        _ => 5500,
    };
    let mut batch: Vec<String> = Vec::new();
    let mut len = 0;
    let mut batches = Vec::new();
    for t in texts {
        let l = t.chars().count();
        if !batch.is_empty() && (len + l > limit || batch.len() >= 100) {
            batches.push(std::mem::take(&mut batch));
            len = 0;
        }
        batch.push(t.clone());
        len += l;
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    for (i, b) in batches.iter().enumerate() {
        if i > 0 {
            // 遵守 QPS 限制
            tokio_sleep(250).await;
        }
        let r = match provider {
            "tencent" => tencent(&client, secret, b, to).await?,
            "baidu" => baidu(&client, secret, b, to).await?,
            "azure" => azure(&client, secret, &cfg.azure_region, b, to).await?,
            "openai" => openai(&client, secret, &cfg.openai_base_url, &cfg.openai_model, b, to).await?,
            _ => bail!("未知的翻译服务: {provider}"),
        };
        out.extend(r);
    }
    Ok(out)
}

async fn tokio_sleep(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

fn hmac256(key: &[u8], msg: &str) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("hmac key");
    m.update(msg.as_bytes());
    m.finalize().into_bytes().to_vec()
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

async fn tencent(
    client: &reqwest::Client,
    secret: &HashMap<String, String>,
    texts: &[String],
    to: &str,
) -> anyhow::Result<Vec<String>> {
    let id = need(secret, "secretId", "腾讯云 SecretId")?;
    let key = need(secret, "secretKey", "腾讯云 SecretKey")?;
    let host = "tmt.tencentcloudapi.com";
    let action = "TextTranslateBatch";
    let payload = json!({
        "SourceTextList": texts,
        "Source": "auto",
        "Target": to,
        "ProjectId": 0
    })
    .to_string();
    let now = chrono::Utc::now();
    let ts = now.timestamp();
    let date = now.format("%Y-%m-%d").to_string();
    // TC3-HMAC-SHA256 签名
    let canonical = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:{host}\nx-tc-action:{}\n\ncontent-type;host;x-tc-action\n{}",
        action.to_lowercase(),
        sha256_hex(&payload)
    );
    let scope = format!("{date}/tmt/tc3_request");
    let to_sign = format!("TC3-HMAC-SHA256\n{ts}\n{scope}\n{}", sha256_hex(&canonical));
    let k_date = hmac256(format!("TC3{key}").as_bytes(), &date);
    let k_service = hmac256(&k_date, "tmt");
    let k_sign = hmac256(&k_service, "tc3_request");
    let signature = hex::encode(hmac256(&k_sign, &to_sign));
    let auth = format!(
        "TC3-HMAC-SHA256 Credential={id}/{scope}, SignedHeaders=content-type;host;x-tc-action, Signature={signature}"
    );
    let resp: Value = client
        .post(format!("https://{host}"))
        .header("Authorization", auth)
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Host", host)
        .header("X-TC-Action", action)
        .header("X-TC-Timestamp", ts.to_string())
        .header("X-TC-Version", "2018-03-21")
        .header("X-TC-Region", "ap-guangzhou")
        .body(payload)
        .send()
        .await
        .context("连接腾讯云失败")?
        .json()
        .await?;
    let r = &resp["Response"];
    if let Some(err) = r.get("Error") {
        bail!(
            "腾讯云返回错误：{} {}",
            err["Code"].as_str().unwrap_or(""),
            err["Message"].as_str().unwrap_or("")
        );
    }
    let list = r["TargetTextList"]
        .as_array()
        .ok_or_else(|| anyhow!("腾讯云返回格式异常"))?;
    Ok(list.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect())
}

async fn baidu(
    client: &reqwest::Client,
    secret: &HashMap<String, String>,
    texts: &[String],
    to: &str,
) -> anyhow::Result<Vec<String>> {
    use md5::Md5;
    let appid = need(secret, "appId", "百度 APP ID")?;
    let key = need(secret, "appKey", "百度密钥")?;
    // 换行分隔，返回结果按行对应
    let lines: Vec<String> = texts.iter().map(|t| t.replace(['\r', '\n'], " ")).collect();
    let q = lines.join("\n");
    let salt = chrono::Utc::now().timestamp_millis().to_string();
    let sign = hex::encode(Md5::digest(format!("{appid}{q}{salt}{key}").as_bytes()));
    let resp: Value = client
        .post("https://fanyi-api.baidu.com/api/trans/vip/translate")
        .form(&[
            ("q", q.as_str()),
            ("from", "auto"),
            ("to", to),
            ("appid", appid),
            ("salt", salt.as_str()),
            ("sign", sign.as_str()),
        ])
        .send()
        .await
        .context("连接百度翻译失败")?
        .json()
        .await?;
    if let Some(code) = resp.get("error_code") {
        bail!(
            "百度翻译返回错误：{} {}",
            code.as_str().map(String::from).unwrap_or_else(|| code.to_string()),
            resp["error_msg"].as_str().unwrap_or("")
        );
    }
    let list = resp["trans_result"]
        .as_array()
        .ok_or_else(|| anyhow!("百度翻译返回格式异常"))?;
    let mut out: Vec<String> = list
        .iter()
        .map(|v| v["dst"].as_str().unwrap_or("").to_string())
        .collect();
    out.resize(texts.len(), String::new());
    Ok(out)
}

async fn azure(
    client: &reqwest::Client,
    secret: &HashMap<String, String>,
    region: &str,
    texts: &[String],
    to: &str,
) -> anyhow::Result<Vec<String>> {
    let key = need(secret, "key", "Azure 密钥")?;
    let to = if to == "zh" { "zh-Hans" } else { to };
    let body: Vec<Value> = texts.iter().map(|t| json!({ "Text": t })).collect();
    let mut req = client
        .post(format!(
            "https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to={to}"
        ))
        .header("Ocp-Apim-Subscription-Key", key)
        .json(&body);
    if !region.trim().is_empty() {
        req = req.header("Ocp-Apim-Subscription-Region", region.trim());
    }
    let resp: Value = req.send().await.context("连接 Azure 失败")?.json().await?;
    if let Some(err) = resp.get("error") {
        bail!(
            "Azure 返回错误：{} {}",
            err["code"],
            err["message"].as_str().unwrap_or("")
        );
    }
    let list = resp.as_array().ok_or_else(|| anyhow!("Azure 返回格式异常"))?;
    Ok(list
        .iter()
        .map(|v| v["translations"][0]["text"].as_str().unwrap_or("").to_string())
        .collect())
}

/// OpenAI 兼容的 /chat/completions 接口：DeepSeek、通义千问、本机 Ollama 等
async fn openai(
    client: &reqwest::Client,
    secret: &HashMap<String, String>,
    base_url: &str,
    model: &str,
    texts: &[String],
    to: &str,
) -> anyhow::Result<Vec<String>> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() || model.trim().is_empty() {
        bail!("请先在设置中填写接口地址和模型名");
    }
    let lang = if to == "zh" { "简体中文" } else { "英文" };
    let prompt = format!(
        "把下面 JSON 数组中的每一项翻译成{lang}。只输出一个 JSON 字符串数组，元素个数和顺序与原数组完全一致，不要输出任何解释。
{}",
        serde_json::to_string(texts)?
    );
    let mut req = client.post(format!("{base}/chat/completions")).json(&json!({
        "model": model.trim(),
        "temperature": 0,
        "stream": false,
        "messages": [
            { "role": "system", "content": "你是专业的翻译引擎，只输出译文。" },
            { "role": "user", "content": prompt }
        ]
    }));
    if let Some(key) = secret.get("key").map(|k| k.trim()).filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.with_context(|| format!("连接 {base} 失败"))?;
    let status = resp.status();
    let body: Value = resp.json().await.context("接口返回的不是 JSON")?;
    if !status.is_success() {
        let msg = body["error"]["message"].as_str().map(String::from).unwrap_or_else(|| body.to_string());
        bail!("接口返回错误（HTTP {status}）：{msg}");
    }
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("接口返回格式异常"))?;
    parse_array(content, texts.len())
}

/// 从模型回复中取出 JSON 数组；只有一项时容忍模型直接输出译文
fn parse_array(content: &str, n: usize) -> anyhow::Result<Vec<String>> {
    // 去掉推理模型的思考过程
    let content = match content.rfind("</think>") {
        Some(i) => &content[i + 8..],
        None => content,
    };
    if let (Some(a), Some(b)) = (content.find('['), content.rfind(']')) {
        if a < b {
            if let Ok(v) = serde_json::from_str::<Vec<String>>(&content[a..=b]) {
                if v.len() == n {
                    return Ok(v);
                }
            }
        }
    }
    if n == 1 {
        return Ok(vec![content.trim().trim_matches('`').trim().to_string()]);
    }
    bail!("模型没有按要求返回逐行译文，请换一个模型或重试")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_model_reply() {
        let r = parse_array("```json
[\"你好\", \"世界\"]
```", 2).unwrap();
        assert_eq!(r, vec!["你好", "世界"]);
        let r = parse_array("<think>嗯</think>[\"a\"]", 1).unwrap();
        assert_eq!(r, vec!["a"]);
        assert_eq!(parse_array("Hello there", 1).unwrap(), vec!["Hello there"]);
        assert!(parse_array("[\"a\"]", 2).is_err());
    }

    /// 用本地假服务器验证 OpenAI 兼容接口的请求与解析
    #[test]
    fn openai_roundtrip() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let mut req = String::new();
            // 读到请求体完整为止
            loop {
                let n = sock.read(&mut buf).unwrap();
                req.push_str(&String::from_utf8_lossy(&buf[..n]));
                if let Some(i) = req.find("\r\n\r\n") {
                    let len: usize = req
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|v| v.trim().parse().unwrap()))
                        .unwrap_or(0);
                    if req.len() >= i + 4 + len {
                        break;
                    }
                }
            }
            let body = r#"{"choices":[{"message":{"role":"assistant","content":"```json\n[\"Hello\", \"World\"]\n```"}}]}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });
        let cfg = crate::settings::Settings {
            openai_base_url: format!("http://127.0.0.1:{port}/v1/"),
            openai_model: "test-model".into(),
            ..Default::default()
        };
        let secret = HashMap::from([("key".to_string(), "sk-test".to_string())]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt
            .block_on(translate("openai", &secret, &cfg, &["你好".into(), "世界".into()], "en"))
            .unwrap();
        assert_eq!(out, vec!["Hello", "World"]);
        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/chat/completions "));
        assert!(req.to_ascii_lowercase().contains("authorization: bearer sk-test"));
        assert!(req.contains("test-model") && req.contains("英文"));
    }
}
