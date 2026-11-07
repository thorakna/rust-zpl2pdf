//! HTTP service converting ZPL to PDF or PNG.
//!
//! Routes:
//! - `POST /v1/printers/{dpmm}dpmm/labels/{w}x{h}/[{index}/]`: Labelary
//!   compatible. The response format follows the `Accept` header
//!   (`application/pdf` by default, `image/png` renders one label).
//! - `POST /v1/convert?dpmm=8&width=4&height=6&format=pdf|png&index=0`.
//! - `GET /health`.
//!
//! When `ZPL2PDF_AUTH_TOKEN` (or `--auth-token`) is set to a non empty value,
//! the conversion routes require `Authorization: Bearer <token>`; `/health`
//! stays open.
//!
//! The request body is the raw ZPL. Rendering runs on blocking threads with
//! a concurrency limit; body size, label count and graphic sizes are limited.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use clap::Parser;
use tokio::sync::Semaphore;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "zpl2pdf-server",
    version,
    about = "HTTP service converting ZPL labels to PDF or PNG"
)]
struct Config {
    /// Address to listen on.
    #[arg(long, env = "ZPL2PDF_LISTEN", default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    /// Maximum request body size in bytes.
    #[arg(long, env = "ZPL2PDF_MAX_BODY", default_value_t = 8 * 1024 * 1024)]
    max_body: usize,
    /// Maximum number of labels per request.
    #[arg(long, env = "ZPL2PDF_MAX_LABELS", default_value_t = 200)]
    max_labels: usize,
    /// Maximum decoded size of one graphic in bytes.
    #[arg(long, env = "ZPL2PDF_MAX_GRAPHIC", default_value_t = 8 * 1024 * 1024)]
    max_graphic: usize,
    /// Number of conversions running at the same time (default: CPU count).
    #[arg(long, env = "ZPL2PDF_CONCURRENCY")]
    concurrency: Option<usize>,
    /// Request timeout in seconds.
    #[arg(long, env = "ZPL2PDF_TIMEOUT", default_value_t = 30)]
    timeout: u64,
    /// Token required as `Authorization: Bearer <token>` on the conversion
    /// routes. Empty or unset disables the check.
    #[arg(long, env = "ZPL2PDF_AUTH_TOKEN", hide_env_values = true)]
    auth_token: Option<String>,
}

impl Config {
    /// The configured token, `None` when authorization is disabled.
    fn auth_token(&self) -> Option<&str> {
        self.auth_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }
}

struct AppState {
    config: Config,
    permits: Semaphore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Pdf,
    Png,
}

struct Request {
    dpmm: u8,
    width: f64,
    height: f64,
    index: Option<usize>,
    format: Format,
}

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message.into(),
    )
        .into_response()
}

/// Compares without an early exit so the time taken does not reveal how many
/// leading bytes of the token matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

/// Accepts `Authorization: Bearer <token>` (scheme case insensitive) or the
/// bare token.
fn is_authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return false;
    };
    let value = value.as_bytes().trim_ascii();
    let credentials = match value.split_at_checked(7) {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case(b"bearer ") => rest.trim_ascii(),
        _ => value,
    };
    let token = token.as_bytes();
    constant_time_eq(value, token) | constant_time_eq(credentials, token)
}

async fn require_auth(
    State(token): State<Arc<str>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    if is_authorized(request.headers(), &token) {
        return next.run(request).await;
    }
    let mut response = error(
        StatusCode::UNAUTHORIZED,
        "missing or invalid Authorization header",
    );
    response
        .headers_mut()
        .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    response
}

fn parse_dpmm(s: &str) -> Result<u8, String> {
    let digits = s.trim_end_matches("dpmm");
    match digits.parse::<u8>() {
        Ok(v @ (6 | 8 | 12 | 24)) => Ok(v),
        _ => Err(format!(
            "invalid print density {s:?}, use 6dpmm, 8dpmm, 12dpmm or 24dpmm"
        )),
    }
}

fn parse_size(s: &str) -> Result<(f64, f64), String> {
    let bad = || format!("invalid label size {s:?}, expected WIDTHxHEIGHT in inches");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    Ok((w.parse().map_err(|_| bad())?, h.parse().map_err(|_| bad())?))
}

fn format_from_accept(headers: &HeaderMap) -> Format {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if accept.contains("image/png") {
        Format::Png
    } else {
        Format::Pdf
    }
}

async fn convert(state: Arc<AppState>, req: Request, body: Bytes) -> Response {
    let opts = zpl2pdf::Options {
        dpmm: req.dpmm,
        width: req.width,
        height: req.height,
        label_index: req.index.or(if req.format == Format::Png {
            Some(0)
        } else {
            None
        }),
        honor_quantity: false,
        limits: zpl2pdf::Limits {
            max_labels: state.config.max_labels,
            max_graphic_bytes: state.config.max_graphic,
            max_input_bytes: state.config.max_body,
        },
    };
    if let Err(e) = opts.validate() {
        return error(StatusCode::BAD_REQUEST, e.to_string());
    }
    let Ok(_permit) = state.permits.acquire().await else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "server is shutting down");
    };
    let format = req.format;
    let result =
        tokio::task::spawn_blocking(move || -> Result<(Vec<u8>, usize, usize), zpl2pdf::Error> {
            let rendered = zpl2pdf::render(
                &body,
                &zpl2pdf::Options {
                    label_index: None,
                    ..opts.clone()
                },
            )?;
            let total = rendered.labels.len();
            let warnings = rendered.warnings.len();
            let labels = match opts.label_index {
                Some(i) if i < total => vec![rendered.labels[i].clone()],
                Some(i) => {
                    return Err(zpl2pdf::Error::LabelIndex {
                        index: i,
                        count: total,
                    });
                }
                None => rendered.labels,
            };
            let data = match format {
                Format::Pdf => zpl2pdf::pdf::write_pdf(&labels, &Default::default())?,
                Format::Png => zpl2pdf::raster::render(&labels[0]).to_png()?,
            };
            Ok((data, total, warnings))
        })
        .await;
    match result {
        Ok(Ok((data, total, warnings))) => {
            let content_type = match format {
                Format::Pdf => "application/pdf",
                Format::Png => "image/png",
            };
            let mut response = (StatusCode::OK, data).into_response();
            let h = response.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            h.insert("X-Total-Count", HeaderValue::from(total));
            h.insert("X-Warnings", HeaderValue::from(warnings));
            response
        }
        Ok(Err(e)) => {
            let status = match e {
                zpl2pdf::Error::Limit(_) => StatusCode::PAYLOAD_TOO_LARGE,
                zpl2pdf::Error::Output(_) => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::BAD_REQUEST,
            };
            error(status, e.to_string())
        }
        Err(e) => {
            tracing::error!("conversion task failed: {e}");
            error(StatusCode::INTERNAL_SERVER_ERROR, "conversion failed")
        }
    }
}

async fn labelary(
    State(state): State<Arc<AppState>>,
    Path((dpmm, size)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    labelary_impl(state, &dpmm, &size, None, &headers, body).await
}

async fn labelary_index(
    State(state): State<Arc<AppState>>,
    Path((dpmm, size, index)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(index) = index.parse::<usize>() else {
        return error(
            StatusCode::BAD_REQUEST,
            format!("invalid label index {index:?}"),
        );
    };
    labelary_impl(state, &dpmm, &size, Some(index), &headers, body).await
}

async fn labelary_impl(
    state: Arc<AppState>,
    dpmm: &str,
    size: &str,
    index: Option<usize>,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    let (dpmm, (width, height)) = match parse_dpmm(dpmm).and_then(|d| Ok((d, parse_size(size)?))) {
        Ok(v) => v,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let format = format_from_accept(headers);
    convert(
        state,
        Request {
            dpmm,
            width,
            height,
            index,
            format,
        },
        body,
    )
    .await
}

#[derive(serde::Deserialize)]
struct ConvertQuery {
    dpmm: Option<u8>,
    width: Option<f64>,
    height: Option<f64>,
    format: Option<String>,
    index: Option<usize>,
}

async fn convert_query(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConvertQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let format = match q.format.as_deref() {
        Some("png") => Format::Png,
        Some("pdf") => Format::Pdf,
        Some(other) => return error(StatusCode::BAD_REQUEST, format!("unknown format {other:?}")),
        None => format_from_accept(&headers),
    };
    let req = Request {
        dpmm: q.dpmm.unwrap_or(8),
        width: q.width.unwrap_or(4.0),
        height: q.height.unwrap_or(6.0),
        index: q.index,
        format,
    };
    convert(state, req, body).await
}

fn app(config: Config) -> Router {
    let concurrency = config.concurrency.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });
    let timeout = Duration::from_secs(config.timeout.max(1));
    let max_body = config.max_body;
    let auth_token: Option<Arc<str>> = config.auth_token().map(Arc::from);
    let state = Arc::new(AppState {
        config,
        permits: Semaphore::new(concurrency.max(1)),
    });
    let conversions = Router::new()
        .route("/v1/printers/{dpmm}/labels/{size}", post(labelary))
        .route("/v1/printers/{dpmm}/labels/{size}/", post(labelary))
        .route(
            "/v1/printers/{dpmm}/labels/{size}/{index}",
            post(labelary_index),
        )
        .route(
            "/v1/printers/{dpmm}/labels/{size}/{index}/",
            post(labelary_index),
        )
        .route("/v1/convert", post(convert_query));
    // `route_layer` runs the check only on matched routes and before the body
    // is read; unknown paths still answer 404.
    let conversions = match auth_token {
        Some(token) => conversions.route_layer(middleware::from_fn_with_state(token, require_auth)),
        None => conversions,
    };
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(conversions)
        .layer(DefaultBodyLimit::max(max_body))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=info".into()),
        )
        .init();
    let config = Config::parse();
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    tracing::info!("listening on {}", config.listen);
    if config.auth_token().is_some() {
        tracing::info!("authorization required on conversion routes");
    } else {
        tracing::warn!("ZPL2PDF_AUTH_TOKEN is empty, conversion routes are open");
    }
    axum::serve(listener, app(config))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn test_config() -> Config {
        Config::parse_from(["zpl2pdf-server", "--concurrency", "2"])
    }

    const ZPL: &str = "^XA^FO50,50^A0N,40,40^FDHello^FS^XZ^XA^FO50,50^A0N,40,40^FDWorld^FS^XZ";

    async fn send(uri: &str, accept: &str, body: &str) -> Response {
        let req = HttpRequest::post(uri)
            .header("Accept", accept)
            .body(Body::from(body.to_string()))
            .unwrap();
        app(test_config()).oneshot(req).await.unwrap()
    }

    fn auth_config(token: &str) -> Config {
        Config::parse_from([
            "zpl2pdf-server",
            "--concurrency",
            "2",
            "--auth-token",
            token,
        ])
    }

    async fn send_auth(config: Config, uri: &str, authorization: Option<&str>) -> Response {
        let mut req = HttpRequest::post(uri).header("Accept", "application/pdf");
        if let Some(value) = authorization {
            req = req.header("Authorization", value);
        }
        app(config)
            .oneshot(req.body(Body::from(ZPL)).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn auth_required_when_token_set() {
        let uris = [
            "/v1/printers/8dpmm/labels/4x6/",
            "/v1/printers/8dpmm/labels/4x6/0/",
            "/v1/convert",
        ];
        for uri in uris {
            let res = send_auth(auth_config("s3cret"), uri, None).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
            assert_eq!(res.headers()["www-authenticate"], "Bearer");
            for bad in [
                "Bearer wrong",
                "Bearer s3cre",
                "Basic s3cret",
                "s3cret2",
                "",
            ] {
                let res = send_auth(auth_config("s3cret"), uri, Some(bad)).await;
                assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri} {bad:?}");
            }
            for good in ["Bearer s3cret", "bearer  s3cret ", "s3cret"] {
                let res = send_auth(auth_config("s3cret"), uri, Some(good)).await;
                assert_eq!(res.status(), StatusCode::OK, "{uri} {good:?}");
            }
        }
        let req = HttpRequest::get("/health").body(Body::empty()).unwrap();
        let res = app(auth_config("s3cret")).oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_disabled_when_token_empty() {
        for token in ["", "   "] {
            let res = send_auth(auth_config(token), "/v1/convert", None).await;
            assert_eq!(res.status(), StatusCode::OK, "{token:?}");
        }
        let res = send_auth(test_config(), "/v1/convert", Some("Bearer anything")).await;
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn labelary_pdf() {
        let res = send("/v1/printers/8dpmm/labels/4x6/", "application/pdf", ZPL).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "application/pdf");
        assert_eq!(res.headers()["x-total-count"], "2");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.starts_with(b"%PDF-"));
    }

    #[tokio::test]
    async fn labelary_png_index() {
        let res = send("/v1/printers/8dpmm/labels/4x6/1/", "image/png", ZPL).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-type"], "image/png");
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(body.starts_with(b"\x89PNG"));
    }

    #[tokio::test]
    async fn convert_route_and_errors() {
        let res = send(
            "/v1/convert?dpmm=12&width=2&height=1&format=png",
            "*/*",
            ZPL,
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let res = send("/v1/printers/7dpmm/labels/4x6/", "application/pdf", ZPL).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send("/v1/printers/8dpmm/labels/4x6/5/", "application/pdf", ZPL).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = send("/v1/convert", "application/pdf", "no labels here").await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
