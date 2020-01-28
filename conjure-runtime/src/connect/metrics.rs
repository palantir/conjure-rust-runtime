use futures::task::{Context, Poll};
use http::Uri;
use hyper::service::Service;
use hyper_openssl::MaybeHttpsStream;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use witchcraft_metrics::{MetricId, MetricRegistry};

struct Shared {
    metrics: Arc<MetricRegistry>,
    service: String,
}

#[derive(Clone)]
pub struct MetricsConnector<T> {
    connector: T,
    shared: Arc<Shared>,
}

impl<T, S> MetricsConnector<T> {
    pub fn new(connector: T, metrics: &Arc<MetricRegistry>, service: &str) -> MetricsConnector<T> {
        MetricsConnector {
            connector,
            shared: Arc::new(Shared {
                metrics: metrics.clone(),
                service: service.to_string(),
            }),
        }
    }
}

impl<T, S> Service<Uri> for MetricsConnector<T>
where
    T: Service<Uri, Response = MaybeHttpsStream<S>>,
    T::Future: 'static + Send,
{
    type Response = MaybeHttpsStream<S>;
    type Error = T::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connector.poll_ready(cx)
    }

    fn call(&mut self, req: Uri) -> Self::Future {
        let shared = self.shared.clone();
        let future = self.connector.call(req);
        Box::pin(async move {
            let stream = future.await?;

            if let MaybeHttpsStream::Https(s) = &stream {
                let cipher = s.ssl().current_cipher().expect("session is active");
                shared
                    .metrics
                    .meter(
                        MetricId::new("tls.handshake")
                            .with_tag("context", shared.service.clone())
                            .with_tag("protocol", s.ssl().version_str())
                            .with_tag(
                                "cipher",
                                cipher.standard_name().unwrap_or_else(|| cipher.name()),
                            ),
                    )
                    .mark(1);
            }

            Ok(stream)
        })
    }
}
