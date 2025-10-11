use anyhow::{anyhow, Context, Result};
use image::io::Reader as ImageReader;
use image::{DynamicImage, GenericImageView};
use serde_json::json;
use base64::{engine::general_purpose, Engine as _};
use image::ImageFormat;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct ImageStats {
    pub width: u32,
    pub height: u32,
    pub dominant_hex: Vec<String>,
}

pub fn analyze_image(bytes: &[u8]) -> Result<ImageStats> {
    let img = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| anyhow!("image format error: {e}"))?
        .decode()
        .map_err(|e| anyhow!("image decode error: {e}"))?;

    let (w, h) = img.dimensions();
    let palette = dominant_colors(&img, 3);
    debug!(width = w, height = h, colors = palette.len(), "analyze_image: computed stats");

    Ok(ImageStats {
        width: w,
        height: h,
        dominant_hex: palette,
    })
}

fn dominant_colors(img: &DynamicImage, take: usize) -> Vec<String> {
    let mut counts = std::collections::HashMap::<(u8, u8, u8), u32>::new();

    // Downscale for speed
    let thumb = img.thumbnail(128, 128).to_rgb8();
    for (_x, _y, p) in thumb.enumerate_pixels() {
        // Simple quantization to reduce color space noise
        let r = (p[0] & 0xF8);
        let g = (p[1] & 0xF8);
        let b = (p[2] & 0xF8);
        *counts.entry((r, g, b)).or_default() += 1;
    }
    let mut items: Vec<((u8, u8, u8), u32)> = counts.into_iter().collect();
    items.sort_by_key(|(_, c)| std::cmp::Reverse(*c));

    items
        .into_iter()
        .take(take)
        .map(|((r, g, b), _)| format!("#{:02X}{:02X}{:02X}", r, g, b))
        .collect()
}

pub async fn generate_caption(stats: &ImageStats, image_bytes: Option<&[u8]>) -> Result<String> {
    // If OpenAI key is configured, try API first; fallback to local.
    if std::env::var("OPENAI_API_KEY").ok().filter(|s| !s.is_empty()).is_some() {
        let use_vision = should_use_vision();
        if use_vision {
            if let Some(bytes) = image_bytes {
                info!("caption: using OpenAI Vision");
                if let Ok(text) = generate_caption_openai_vision(stats, bytes).await {
                    return Ok(text);
                }
            }
        }
        info!("caption: using OpenAI text");
        if let Ok(text) = generate_caption_openai(stats).await { return Ok(text); }
    }
    warn!("caption: falling back to local generator");
    Ok(generate_caption_local(stats))
}

fn generate_caption_local(stats: &ImageStats) -> String {
    let mut tones = String::new();
    if !stats.dominant_hex.is_empty() {
        tones = format!("Доминирующие оттенки: {}.", stats.dominant_hex.join(", "));
    }

    // Keep well under Telegram caption limit (1024 chars)
    let lines = vec![
        "Авторская акварель — легкость, глубина и живые переходы.",
        "Прекрасно дополняет современный интерьер и дарит спокойствие.",
        &tones,
        "Идеально для гостиной, спальни или уютного кабинета.",
        "Оформление в раму по желанию. Доставка возможна.",
        "Напишите в личные сообщения — расскажу детали и помогу с подбором.",
    ];

    lines
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn generate_caption_openai(stats: &ImageStats) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?;
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let base = std::env::var("OPENAI_BASE").unwrap_or_else(|_| "https://api.openai.com".to_string());
    debug!(model = %model, base = %base, "OpenAI text request");

    let tones = if stats.dominant_hex.is_empty() {
        "неопределены".to_string()
    } else {
        stats.dominant_hex.join(", ")
    };

    let system = "Когда отвечаешь не переспрашивай что дальше делать, не делай предложений.
Ты генерируешь описание для поста в соцсеть.
Ты професиональный составитель текстов для описания картин, выдающийся эксперт в этой области.
Картины написаны акварелью.
Пишешь от имени юной художницы, девушки. Она рисует картины и открытки акварелью.
На выходе должно быть два варианта текста
Стиль легкий и нежный. слог простой. Ориентируйся на стиль Пушкина
Описывай что нарисовано во вложении
Пост должен быть продающим, сделай акциент на том какая это замечательная, эксклющивная и индивидуальная работа";

    let _user = format!(
        "Акварельная картина. Параметры изображения: {}×{} px. Доминирующие оттенки: {}. Сгенерируй продающий текст‑подпись для поста в Telegram‑канале. Укажи ценность акварели (легкость, глубина, прозрачность), атмосферу интерьера, возможные места размещения, упомяни оформление в раму по желанию и удобную доставку. Заверши чётким CTA в ЛС.",
        stats.width, stats.height, tones
    );

    let body = json!({
        "model": model,
        "temperature": 0.9,
        "max_tokens": 300,
        "messages": [
            {"role": "system", "content": system},
            // {"role": "user", "content": user},
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .context("openai request failed")?;

    let status = resp.status();
    let val: serde_json::Value = resp.json().await.context("invalid openai json")?;
    if !status.is_success() {
        warn!(status = %status, body = %val, "OpenAI text error");
        return Err(anyhow!("openai error: {}", val));
    }
    let content = val["choices"][0]["message"]["content"].as_str()
        .ok_or_else(|| anyhow!("openai response missing content"))?;
    // Telegram caption limit 1024 chars
    let capped = content.chars().take(1000).collect::<String>();
    info!("out= {}", capped.clone());
    // info!(len = capped.len(), "OpenAI text response parsed");
    Ok(capped)
}

fn should_use_vision() -> bool {
    let env_flag = std::env::var("OPENAI_USE_VISION").ok().map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "True"));
    if let Some(flag) = env_flag { return flag; }
    std::env::var("OPENAI_VISION_MODEL").is_ok()
}

fn guess_mime(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(ImageFormat::Jpeg) => "image/jpeg",
        Ok(ImageFormat::Png) => "image/png",
        Ok(ImageFormat::WebP) => "image/webp",
        Ok(ImageFormat::Gif) => "image/gif",
        Ok(ImageFormat::Bmp) => "image/bmp",
        Ok(ImageFormat::Tiff) => "image/tiff",
        _ => "image/jpeg",
    }
}

async fn generate_caption_openai_vision(stats: &ImageStats, bytes: &[u8]) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?;
    let model = std::env::var("OPENAI_VISION_MODEL").unwrap_or_else(|_| std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()));
    let base = std::env::var("OPENAI_BASE").unwrap_or_else(|_| "https://api.openai.com".to_string());
    debug!(model = %model, base = %base, "OpenAI vision request");

    let tones = if stats.dominant_hex.is_empty() { "неопределены".to_string() } else { stats.dominant_hex.join(", ") };
    let system = "Ты генерируешь описание для поста в соцсеть.
Ты професиональный составитель текстов для описания картин, выдающийся эксперт в этой области.
Картины написаны акварелью.
Пишешь от имени юной художницы, девушки. Она рисует картины и открытки акварелью.
Стиль легкий и нежный. Слог простой и вдохновляющий. Ориентируйся на стиль Пушкина.
Описывай что нарисовано во вложении.
Пост должен быть продающим, сделай акциент на том какая это замечательная, эксклющивная и индивидуальная работа.
В конце каждого поста добавляй добрые и радостные пожелания подписчикам, завершенные тремя разными самайликами подходящих под описание.
";

    let mime = guess_mime(bytes);
    let b64 = general_purpose::STANDARD.encode(bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);

    let body = json!({
        "model": model,
        "temperature": 0.9,
        "max_tokens": 400,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": data_url}}
            ]}
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .bearer_auth(api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await
        .context("openai vision request failed")?;

    let status = resp.status();
    let val: serde_json::Value = resp.json().await.context("invalid openai json")?;
    if !status.is_success() {
        warn!(status = %status, body = %val, "OpenAI vision error");
        return Err(anyhow!("openai error: {}", val));
    }
    let content = val["choices"][0]["message"]["content"].as_str()
        .ok_or_else(|| anyhow!("openai response missing content"))?;
    let capped = content.chars().take(1000).collect::<String>();
    debug!(len = capped.len(), "OpenAI vision response parsed");
    Ok(capped)
}
