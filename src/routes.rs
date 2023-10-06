use hyper::{header::CONTENT_TYPE, Body, Method, Request, Response, StatusCode};
use prometheus::{Encoder, TextEncoder};
use routerify::ext::RequestExt;
use routerify::{RouteError, Router, RouterBuilder};
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::future::Future;
use std::sync::Arc;
use thiserror::Error;
use tracing::{self, debug, error, info, info_span, Instrument};

use crate::metrics;
use crate::postgres_connection::PgConnectionConfig;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Bad request: {0:#?}")]
    BadRequest(anyhow::Error),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("NotFound: {0}")]
    NotFound(Box<dyn StdError + Send + Sync + 'static>),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Precondition failed: {0}")]
    PreconditionFailed(Box<str>),

    #[error(transparent)]
    InternalServerError(anyhow::Error),
}

impl ApiError {
    pub fn into_response(self) -> Response<Body> {
        match self {
            ApiError::BadRequest(err) => HttpErrorBody::response_from_msg_and_status(
                format!("{err:#?}"), // use debug printing so that we give the cause
                StatusCode::BAD_REQUEST,
            ),
            ApiError::Forbidden(_) => {
                HttpErrorBody::response_from_msg_and_status(self.to_string(), StatusCode::FORBIDDEN)
            }
            ApiError::Unauthorized(_) => HttpErrorBody::response_from_msg_and_status(
                self.to_string(),
                StatusCode::UNAUTHORIZED,
            ),
            ApiError::NotFound(_) => {
                HttpErrorBody::response_from_msg_and_status(self.to_string(), StatusCode::NOT_FOUND)
            }
            ApiError::Conflict(_) => {
                HttpErrorBody::response_from_msg_and_status(self.to_string(), StatusCode::CONFLICT)
            }
            ApiError::PreconditionFailed(_) => HttpErrorBody::response_from_msg_and_status(
                self.to_string(),
                StatusCode::PRECONDITION_FAILED,
            ),
            ApiError::InternalServerError(err) => HttpErrorBody::response_from_msg_and_status(
                err.to_string(),
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct HttpErrorBody {
    pub msg: String,
}

impl HttpErrorBody {
    pub fn response_from_msg_and_status(msg: String, status: StatusCode) -> Response<Body> {
        HttpErrorBody { msg }.to_response(status)
    }

    pub fn to_response(&self, status: StatusCode) -> Response<Body> {
        Response::builder()
            .status(status)
            .header(CONTENT_TYPE, "application/json")
            // we do not have nested maps with non string keys so serialization shouldn't fail
            .body(Body::from(serde_json::to_string(self).unwrap()))
            .unwrap()
    }
}

#[derive(Debug, Default, Clone)]
struct RequestId(String);

struct RequestCancelled {
    warn: Option<tracing::Span>,
}

impl RequestCancelled {
    /// Create the drop guard using the [`tracing::Span::current`] as the span.
    fn warn_when_dropped_without_responding() -> Self {
        RequestCancelled {
            warn: Some(tracing::Span::current()),
        }
    }

    /// Consume the drop guard without logging anything.
    fn disarm(mut self) {
        self.warn = None;
    }
}

/// Adds a tracing info_span! instrumentation around the handler events,
/// logs the request start and end events for non-GET requests and non-200 responses.
///
/// Usage: Replace `my_handler` with `|r| request_span(r, my_handler)`
///
/// Use this to distinguish between logs of different HTTP requests: every request handler wrapped
/// with this will get request info logged in the wrapping span, including the unique request ID.
///
/// This also handles errors, logging them and converting them to an HTTP error response.
///
/// NB: If the client disconnects, Hyper will drop the Future, without polling it to
/// completion. In other words, the handler must be async cancellation safe! request_span
/// prints a warning to the log when that happens, so that you have some trace of it in
/// the log.
///
///
/// There could be other ways to implement similar functionality:
///
/// * procmacros placed on top of all handler methods
/// With all the drawbacks of procmacros, brings no difference implementation-wise,
/// and little code reduction compared to the existing approach.
///
/// * Another `TraitExt` with e.g. the `get_with_span`, `post_with_span` methods to do similar logic,
/// implemented for [`RouterBuilder`].
/// Could be simpler, but we don't want to depend on [`routerify`] more, targeting to use other library later.
///
/// * In theory, a span guard could've been created in a pre-request middleware and placed into a global collection, to be dropped
/// later, in a post-response middleware.
/// Due to suspendable nature of the futures, would give contradictive results which is exactly the opposite of what `tracing-futures`
/// tries to achive with its `.instrument` used in the current approach.
///
/// If needed, a declarative macro to substitute the |r| ... closure boilerplate could be introduced.
async fn request_span<R, H>(request: Request<Body>, handler: H) -> R::Output
where
    R: Future<Output = Result<Response<Body>, ApiError>> + Send + 'static,
    H: FnOnce(Request<Body>) -> R + Send + Sync + 'static,
{
    let request_id = request.context::<RequestId>().unwrap_or_default().0;
    let method = request.method();
    let path = request.uri().path();
    let request_span = info_span!("request", %method, %path, %request_id);

    let log_quietly = method == Method::GET;
    async move {
        let cancellation_guard = RequestCancelled::warn_when_dropped_without_responding();
        if log_quietly {
            debug!("Handling request");
        } else {
            info!("Handling request");
        }

        // No special handling for panics here. There's a `tracing_panic_hook` from another
        // module to do that globally.
        let res = handler(request).await;

        cancellation_guard.disarm();

        // Log the result if needed.
        //
        // We also convert any errors into an Ok response with HTTP error code here.
        // `make_router` sets a last-resort error handler that would do the same, but
        // we prefer to do it here, before we exit the request span, so that the error
        // is still logged with the span.
        //
        // (Because we convert errors to Ok response, we never actually return an error,
        // and we could declare the function to return the never type (`!`). However,
        // using `routerify::RouterBuilder` requires a proper error type.)
        match res {
            Ok(response) => {
                let response_status = response.status();
                if log_quietly && response_status.is_success() {
                    debug!("Request handled, status: {response_status}");
                } else {
                    info!("Request handled, status: {response_status}");
                }
                Ok(response)
            }
            Err(err) => Ok(api_error_handler(err)),
        }
    }
    .instrument(request_span)
    .await
}

pub fn make_router(state: Arc<State>) -> anyhow::Result<RouterBuilder<hyper::Body, ApiError>> {
    let router = Router::builder()
        .data(state)
        .get("/metrics", |r| request_span(r, prometheus_metrics_handler))
        .err_handler(route_error_handler);

    Ok(router)
}

pub struct State {
    pub pgnode: &'static PgConnectionConfig,
}

#[inline(always)]
fn get_state(request: &Request<Body>) -> &State {
    request
        .data::<Arc<State>>()
        .expect("unknown state type")
        .as_ref()
}

async fn prometheus_metrics_handler(_req: Request<Body>) -> Result<Response<Body>, ApiError> {
    use bytes::{Bytes, BytesMut};
    use std::io::Write as _;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    // SERVE_METRICS_COUNT.inc();

    /// An [`std::io::Write`] implementation on top of a channel sending [`bytes::Bytes`] chunks.
    struct ChannelWriter {
        buffer: BytesMut,
        tx: mpsc::Sender<std::io::Result<Bytes>>,
        written: usize,
    }

    impl ChannelWriter {
        fn new(buf_len: usize, tx: mpsc::Sender<std::io::Result<Bytes>>) -> Self {
            assert_ne!(buf_len, 0);
            ChannelWriter {
                // split about half off the buffer from the start, because we flush depending on
                // capacity. first flush will come sooner than without this, but now resizes will
                // have better chance of picking up the "other" half. not guaranteed of course.
                buffer: BytesMut::with_capacity(buf_len).split_off(buf_len / 2),
                tx,
                written: 0,
            }
        }

        fn flush0(&mut self) -> std::io::Result<usize> {
            let n = self.buffer.len();
            if n == 0 {
                return Ok(0);
            }

            tracing::trace!(n, "flushing");
            let ready = self.buffer.split().freeze();

            // not ideal to call from blocking code to block_on, but we are sure that this
            // operation does not spawn_blocking other tasks
            let res: Result<(), ()> = tokio::runtime::Handle::current().block_on(async {
                self.tx.send(Ok(ready)).await.map_err(|_| ())?;

                // throttle sending to allow reuse of our buffer in `write`.
                self.tx.reserve().await.map_err(|_| ())?;

                // now the response task has picked up the buffer and hopefully started
                // sending it to the client.
                Ok(())
            });
            if res.is_err() {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
            self.written += n;
            Ok(n)
        }

        fn flushed_bytes(&self) -> usize {
            self.written
        }
    }

    impl std::io::Write for ChannelWriter {
        fn write(&mut self, mut buf: &[u8]) -> std::io::Result<usize> {
            let remaining = self.buffer.capacity() - self.buffer.len();

            let out_of_space = remaining < buf.len();

            let original_len = buf.len();

            if out_of_space {
                let can_still_fit = buf.len() - remaining;
                self.buffer.extend_from_slice(&buf[..can_still_fit]);
                buf = &buf[can_still_fit..];
                self.flush0()?;
            }

            // assume that this will often under normal operation just move the pointer back to the
            // beginning of allocation, because previous split off parts are already sent and
            // dropped.
            self.buffer.extend_from_slice(buf);
            Ok(original_len)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flush0().map(|_| ())
        }
    }

    let started_at = std::time::Instant::now();

    let (tx, rx) = mpsc::channel(1);

    let body = hyper::Body::wrap_stream(ReceiverStream::new(rx));

    let mut writer = ChannelWriter::new(128 * 1024, tx);

    let encoder = TextEncoder::new();

    let response = Response::builder()
        .status(200)
        .header(CONTENT_TYPE, encoder.format_type())
        .body(body)
        .unwrap();

    let span = info_span!("blocking");
    tokio::task::spawn_blocking(move || {
        let _span = span.entered();
        let metrics = metrics::gather(get_state(&_req).pgnode);
        let res = encoder
            .encode(&metrics, &mut writer)
            .and_then(|_| writer.flush().map_err(|e| e.into()));

        match res {
            Ok(()) => {
                tracing::info!(
                    bytes = writer.flushed_bytes(),
                    elapsed_ms = started_at.elapsed().as_millis(),
                    "responded /metrics"
                );
            }
            Err(e) => {
                tracing::warn!("failed to write out /metrics response: {e:#}");
                // semantics of this error are quite... unclear. we want to error the stream out to
                // abort the response to somehow notify the client that we failed.
                //
                // though, most likely the reason for failure is that the receiver is already gone.
                drop(
                    writer
                        .tx
                        .blocking_send(Err(std::io::ErrorKind::BrokenPipe.into())),
                );
            }
        }
    });

    Ok(response)
}

async fn route_error_handler(err: RouteError) -> Response<Body> {
    match err.downcast::<ApiError>() {
        Ok(api_error) => api_error_handler(*api_error),
        Err(other_error) => {
            // We expect all the request handlers to return an ApiError, so this should
            // not be reached. But just in case.
            error!("Error processing HTTP request: {other_error:?}");
            HttpErrorBody::response_from_msg_and_status(
                other_error.to_string(),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        }
    }
}

fn api_error_handler(api_error: ApiError) -> Response<Body> {
    // Print a stack trace for Internal Server errors
    if let ApiError::InternalServerError(_) = api_error {
        error!("Error processing HTTP request: {api_error:?}");
    } else {
        error!("Error processing HTTP request: {api_error:#}");
    }

    api_error.into_response()
}
