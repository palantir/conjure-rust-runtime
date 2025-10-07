// Copyright 2025 Palantir Technologies, Inc.
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
use crate::service::raw::{RawRequestBody, RequestBodyError};
use crate::service::Service;
use crate::{builder, Builder};
use bytes::{Bytes, BytesMut};
use conjure_error::Error;
use futures::TryStreamExt;
use http::{header, HeaderName, HeaderValue, Request, Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use js_sys::{Array, JsString, Promise, Uint8Array};
use std::convert::TryFrom;
use std::future::Future;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use std::{error, fmt};
use wasm_bindgen::prelude::{wasm_bindgen, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Headers, ReadableStreamDefaultReader, ReadableStreamReadResult, RequestInit,
};

const MAX_BODY_SIZE: usize = 50 * 1024 * 1024;
static FETCH_USER_AGENT: HeaderName = HeaderName::from_static("fetch-user-agent");

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = fetch)]
    fn fetch_with_request(input: &web_sys::Request) -> Promise;
}

pub struct RawClient(());

impl RawClient {
    pub fn new(_: &Builder<builder::Complete>) -> Result<Self, Error> {
        Ok(RawClient(()))
    }
}

impl Service<Request<RawRequestBody>> for RawClient {
    type Response = Response<RawResponseBody>;
    type Error = Box<dyn error::Error + Sync + Send>;

    // The fetch API promises to call these futures sequentially
    #[allow(clippy::await_holding_refcell_ref)]
    async fn call(&self, req: Request<RawRequestBody>) -> Result<Self::Response, Self::Error> {
        let (parts, body) = req.into_parts();

        let init = RequestInit::new();
        init.set_method(parts.method.as_str());

        let headers = Headers::new().map_err(JsError::new)?;
        for (mut name, value) in &parts.headers {
            if name == header::USER_AGENT {
                name = &FETCH_USER_AGENT;
            }

            headers
                .append(name.as_str(), value.to_str().map_err(|e| e.to_string())?)
                .map_err(JsError::new)?;
        }

        init.set_headers(headers.as_ref());

        // We're buffering the entire body because ReadableStreams are not supported in Safari and
        // Firefox today. They are supported in Chrome (paired with 'duplex: half'). We'll just
        // buffer the body since it's compatible with every browser, and it's not simple to
        // determine at runtime which browser we're running in.
        if let Some(data) = read_body(body, MAX_BODY_SIZE).await? {
            let js_array = Uint8Array::from(&data[..]);
            init.set_body(&js_array.into());
        };

        let abort_controller = AbortController::new().map_err(JsError::new)?;
        init.set_signal(Some(&abort_controller.signal()));

        let guard = AbortGuard { abort_controller };

        let request = web_sys::Request::new_with_str_and_init(&parts.uri.to_string(), &init)
            .map_err(JsError::new)?;

        let response = JsFuture::from(fetch_with_request(&request))
            .await
            .map_err(JsError::new)?;
        let response = response.unchecked_into::<web_sys::Response>();

        let body = RawResponseBody {
            reader: response.body().map(|s| {
                s.get_reader()
                    .unchecked_into::<ReadableStreamDefaultReader>()
            }),
            pending: None,
            _guard: guard,
        };
        let mut resp = Response::new(body);

        *resp.status_mut() = StatusCode::from_u16(response.status())?;

        for pair in response.headers().entries() {
            let pair = pair.map_err(JsError::new)?;
            let pair = pair.unchecked_into::<Array>();

            let name = ToString::to_string(&pair.at(0).unchecked_into::<JsString>());
            let name = HeaderName::try_from(name)?;
            let value = ToString::to_string(&pair.at(1).unchecked_into::<JsString>());
            let value = HeaderValue::try_from(value)?;

            resp.headers_mut().append(name, value);
        }

        Ok(resp)
    }
}

struct AbortGuard {
    abort_controller: AbortController,
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        self.abort_controller.abort();
    }
}

pub struct RawResponseBody {
    reader: Option<ReadableStreamDefaultReader>,
    pending: Option<JsFuture>,
    _guard: AbortGuard,
}

impl Body for RawResponseBody {
    type Data = Bytes;
    type Error = JsError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;

        let pending = match &mut this.pending {
            Some(pending) => pending,
            None => match &this.reader {
                Some(reader) => this.pending.insert(JsFuture::from(reader.read())),
                None => return Poll::Ready(None),
            },
        };

        let chunk = ready!(Pin::new(pending).poll(cx)).map_err(JsError::new)?;
        this.pending = None;

        let chunk = ReadableStreamReadResult::from(chunk);

        if chunk.get_done() == Some(true) {
            return Poll::Ready(None);
        }

        let chunk = chunk.get_value().unchecked_into::<Uint8Array>();

        Poll::Ready(Some(Ok(Frame::data(Bytes::from(chunk.to_vec())))))
    }
}

async fn read_body(body: RawRequestBody, limit: usize) -> Result<Option<Bytes>, JsError> {
    let mut data_stream = body.into_data_stream();

    let first = match data_stream.try_next().await? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };
    check_limit(&first, limit)?;

    let mut buf = BytesMut::new();
    match data_stream.try_next().await? {
        Some(second) => {
            buf.reserve(first.len() + second.len());
            buf.extend_from_slice(&first);
            buf.extend_from_slice(&second);
        }
        None => return Ok(Some(first)),
    }
    check_limit(&buf, limit)?;

    while let Some(bytes) = data_stream.try_next().await? {
        buf.extend_from_slice(&bytes);
        check_limit(&buf, limit)?;
    }

    Ok(Some(buf.freeze()))
}

fn check_limit(buf: &[u8], limit: usize) -> Result<(), JsError> {
    if buf.len() > limit {
        return Err(JsError::new(JsString::from("body too large").into()));
    }
    Ok(())
}

#[derive(Debug)]
pub struct JsError(String);

impl From<RequestBodyError> for JsError {
    fn from(value: RequestBodyError) -> Self {
        JsError(value.to_string())
    }
}

impl JsError {
    fn new(raw: JsValue) -> Self {
        JsError(format!("{raw:?}"))
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, fmt)
    }
}

impl error::Error for JsError {}
