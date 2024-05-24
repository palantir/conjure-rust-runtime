// Copyright 2020 Palantir Technologies, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use crate::blocking::{body, BodyWriter, BodyWriterShim, ResponseBody};
use crate::raw::{DefaultRawClient, RawBody, Service};
use crate::{builder, Builder};
use bytes::Bytes;
use conjure_error::Error;
use conjure_http::client::{self, AsyncClient, AsyncRequestBody, BoxAsyncWriteBody, RequestBody};
use futures::channel::oneshot;
use futures::executor;
use http::{Request, Response};
use once_cell::sync::OnceCell;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{error, io};
use tokio::runtime::{self, Handle, Runtime};
use zipkin::TraceContext;

fn default_handle() -> io::Result<&'static Handle> {
    static RUNTIME: OnceCell<Runtime> = OnceCell::new();
    RUNTIME
        .get_or_try_init(|| {
            runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("conjure-runtime")
                .build()
        })
        .map(Runtime::handle)
}

/// A blocking HTTP client to a remote service.
///
/// It implements the Conjure `Client` trait, but also offers a "raw" request interface for use with services that don't
/// provide Conjure service definitions.
pub struct Client<T = DefaultRawClient> {
    pub(crate) client: crate::Client<T>,
    pub(crate) handle: Option<Handle>,
}

impl<T> Clone for Client<T> {
    fn clone(&self) -> Self {
        Client {
            client: self.client.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl Client {
    /// Returns a new `Builder` for clients.
    #[inline]
    pub fn builder() -> Builder<builder::ServiceStage> {
        Builder::new()
    }
}

impl<T> client::Service<Client<T>> for Client<T> {
    fn new(client: Client<T>) -> Self {
        client
    }
}

impl<T, B> conjure_http::client::Client for Client<T>
where
    T: Service<Request<RawBody>, Response = Response<B>> + 'static + Sync + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    B: http_body::Body<Data = Bytes> + 'static + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type BodyWriter = BodyWriter;

    type ResponseBody = ResponseBody<B>;

    fn send(
        &self,
        req: Request<RequestBody<'_, Self::BodyWriter>>,
    ) -> Result<Response<Self::ResponseBody>, Error> {
        let mut streamer = None;
        let req = req.map(|body| match body {
            RequestBody::Empty => ShimBody::Empty,
            RequestBody::Fixed(bytes) => ShimBody::Fixed(bytes),
            RequestBody::Streaming(body_writer) => {
                let shim = body::shim(body_writer);
                streamer = Some(shim.1);
                ShimBody::Streaming(shim.0)
            }
        });

        let handle = match &self.handle {
            Some(handle) => handle,
            None => default_handle().map_err(Error::internal_safe)?,
        };

        let (sender, receiver) = oneshot::channel();

        handle.spawn(ContextFuture::new({
            let client = self.client.clone();
            async move {
                let (parts, body) = req.into_parts();
                let body = match body {
                    ShimBody::Empty => AsyncRequestBody::Empty,
                    ShimBody::Fixed(bytes) => AsyncRequestBody::Fixed(bytes),
                    ShimBody::Streaming(writer) => {
                        AsyncRequestBody::Streaming(BoxAsyncWriteBody::new(writer))
                    }
                };
                let req = Request::from_parts(parts, body);

                let r = client.send(req).await;
                let _ = sender.send(r);
            }
        }));

        if let Some(streamer) = streamer {
            streamer.stream();
        }

        match executor::block_on(receiver) {
            Ok(Ok(r)) => Ok(r.map(|body| ResponseBody::new(body, handle.clone()))),
            Ok(Err(e)) => Err(e.with_backtrace()),
            Err(e) => Err(Error::internal_safe(e)),
        }
    }
}

enum ShimBody {
    Empty,
    Fixed(Bytes),
    Streaming(BodyWriterShim),
}

#[pin_project]
struct ContextFuture<F> {
    #[pin]
    future: F,
    context: Option<TraceContext>,
}

impl<F> ContextFuture<F>
where
    F: Future,
{
    fn new(future: F) -> ContextFuture<F> {
        ContextFuture {
            future,
            context: zipkin::current(),
        }
    }
}

impl<F> Future for ContextFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.project();
        let _guard = this.context.map(zipkin::set_current);
        this.future.poll(cx)
    }
}
