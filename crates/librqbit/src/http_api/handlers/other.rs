use std::time::Duration;

use anyhow::Context;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
};
use bencode::AsDisplay;
use buffers::ByteBuf;
use http::{HeaderMap, HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::ApiState;
use crate::{
    AddTorrent, AddTorrentOptions, ListOnlyResponse, WithStatus, api::Result,
    http_api::timeout::Timeout,
};

const TORRENT_SEARCH_API_BASE: &str = "http://127.0.0.1:8080/api";

#[derive(Deserialize)]
pub struct TorrentSearchPathParams {
    website: String,
    query: String,
}

#[derive(Deserialize)]
pub struct TorrentSearchPathParamsWithPage {
    website: String,
    query: String,
    page: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TorrentSearchResult {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Size")]
    size: String,
    #[serde(rename = "DateUploaded")]
    date_uploaded: String,
    #[serde(rename = "Category")]
    category: String,
    #[serde(rename = "Seeders")]
    seeders: String,
    #[serde(rename = "Leechers")]
    leechers: String,
    #[serde(rename = "UploadedBy")]
    uploaded_by: String,
    #[serde(rename = "Url")]
    url: String,
    #[serde(rename = "Magnet")]
    magnet: String,
}

fn validate_torrent_search_website(website: &str) -> Result<&str> {
    match website {
        "piratebay" => Ok("piratebay"),
        "rargb" | "rarbg" => Ok("rarbg"),
        "ettv" => Ok("ettv"),
        "zooqle" => Ok("zooqle"),
        "kickass" => Ok("kickass"),
        "torrentproject" => Ok("torrentproject"),
        _ => Err((StatusCode::BAD_REQUEST, "unsupported torrent search website").into()),
    }
}

fn value_to_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => String::new(),
    }
}

fn get_string_field(object: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| object.get(*key))
        .map(|value| value_to_string(Some(value)))
        .unwrap_or_default()
}

fn normalize_torrent_search_result(value: Value) -> Option<TorrentSearchResult> {
    let object = value.as_object()?;

    Some(TorrentSearchResult {
        name: get_string_field(object, &["Name", "name", "title"]),
        size: get_string_field(object, &["Size", "size"]),
        date_uploaded: get_string_field(object, &["DateUploaded", "Age", "date_uploaded"]),
        category: get_string_field(object, &["Category", "category"]),
        seeders: get_string_field(object, &["Seeders", "seeders"]),
        leechers: get_string_field(object, &["Leechers", "leechers"]),
        uploaded_by: get_string_field(object, &["UploadedBy", "uploaded_by", "uploader"]),
        url: get_string_field(object, &["Url", "url"]),
        magnet: get_string_field(object, &["Magnet", "magnet"]),
    })
}

fn parse_torrent_search_body(body: &str) -> Result<Vec<TorrentSearchResult>> {
    let json: Value = serde_json::from_str(body).context("error parsing torrent search response")?;

    match json {
        Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(normalize_torrent_search_result)
            .collect()),
        Value::Object(object) => {
            if let Some(error) = object.get("error").and_then(Value::as_str) {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    anyhow::anyhow!("torrent search upstream error: {error}"),
                )
                    .into());
            }

            for key in ["results", "data", "items"] {
                if let Some(Value::Array(items)) = object.get(key) {
                    return Ok(items
                        .iter()
                        .cloned()
                        .filter_map(normalize_torrent_search_result)
                        .collect());
                }
            }

            Err((
                StatusCode::BAD_GATEWAY,
                anyhow::anyhow!("torrent search upstream returned an unsupported JSON object"),
            )
                .into())
        }
        _ => Err((
            StatusCode::BAD_GATEWAY,
            anyhow::anyhow!("torrent search upstream returned unsupported JSON"),
        )
            .into()),
    }
}

async fn torrent_search_request(
    website: &str,
    query: &str,
    page: Option<u32>,
) -> Result<axum::Json<Vec<TorrentSearchResult>>> {
    let website = validate_torrent_search_website(website)?;

    let mut url = reqwest::Url::parse(TORRENT_SEARCH_API_BASE)
        .context("invalid torrent search API base URL")?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("could not build torrent search URL"))?;
        segments.push(website);
        segments.push(query);
        if let Some(page) = page {
            segments.push(&page.to_string());
        }
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("error building torrent search client")?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_GATEWAY,
                anyhow::anyhow!("error querying torrent search upstream: {error}"),
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            anyhow::anyhow!("torrent search upstream returned {status}: {body}"),
        )
            .into());
    }

    let body = response
        .text()
        .await
        .context("error reading torrent search response body")?;

    let results = parse_torrent_search_body(&body)?;

    Ok(axum::Json(results))
}

pub async fn h_torrent_search(
    Path(TorrentSearchPathParams { website, query }): Path<TorrentSearchPathParams>,
) -> Result<impl IntoResponse> {
    torrent_search_request(&website, &query, None).await
}

pub async fn h_torrent_search_with_page(
    Path(TorrentSearchPathParamsWithPage {
        website,
        query,
        page,
    }): Path<TorrentSearchPathParamsWithPage>,
) -> Result<impl IntoResponse> {
    torrent_search_request(&website, &query, Some(page)).await
}

pub async fn h_resolve_magnet(
    State(state): State<ApiState>,
    Timeout(timeout): Timeout<600_000, 3_600_000>,
    inp_headers: HeaderMap,
    url: String,
) -> Result<impl IntoResponse> {
    let added = tokio::time::timeout(
        timeout,
        state.api.session().add_torrent(
            AddTorrent::from_url(&url),
            Some(AddTorrentOptions {
                list_only: true,
                ..Default::default()
            }),
        ),
    )
    .await
    .context("timeout")??;

    let (info, content) = match added {
        crate::AddTorrentResponse::AlreadyManaged(_, handle) => {
            handle.with_metadata(|r| (r.info.clone(), r.torrent_bytes.clone()))?
        }
        crate::AddTorrentResponse::ListOnly(ListOnlyResponse {
            info,
            torrent_bytes,
            ..
        }) => (info, torrent_bytes),
        crate::AddTorrentResponse::Added(_, _) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "bug: torrent was added to session, but shouldn't have been",
            )
                .into());
        }
    };

    let mut headers = HeaderMap::new();

    if inp_headers
        .get("Accept")
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        == Some("application/json")
    {
        let data = bencode::dyn_from_bytes::<AsDisplay<ByteBuf>>(&content)
            .map_err(|e| {
                tracing::trace!("error decoding .torrent file content: {e:#}");
                e.into_kind()
            })
            .context("error decoding .torrent file content")?;
        let data = serde_json::to_string(&data).context("error serializing")?;
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        return Ok((headers, data).into_response());
    }

    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-bittorrent"),
    );

    if let Some(name) = info.name()
        && let Ok(h) = HeaderValue::from_str(&format!("attachment; filename=\"{name}.torrent\""))
    {
        headers.insert("Content-Disposition", h);
    }
    Ok((headers, content).into_response())
}
