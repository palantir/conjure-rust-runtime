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
use crate::{
    Body, BodyWriter, Client, RequestBuilder, Response, ResponseBody, APPLICATION_JSON,
    APPLICATION_OCTET_STREAM,
};
use async_trait::async_trait;
use bytes::Bytes;
use conjure_error::Error;
use conjure_http::client::{
    Accept, AsyncClient, AsyncRequestBody, AsyncVisitRequestBody, AsyncWriteBody, VisitResponse,
};
use conjure_http::{PathParams, QueryParams};
use conjure_serde::json;
use futures::pin_mut;
use hyper::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
use hyper::{HeaderMap, Method, StatusCode};
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;
use tokio::io::AsyncReadExt;

impl Client {
    #[allow(clippy::too_many_arguments)]
    async fn conjure_inner(
        &self,
        method: Method,
        path: &str,
        path_params: PathParams,
        query_params: QueryParams,
        headers: HeaderMap,
        body: Option<RawBody<'_>>,
        accept: Accept,
    ) -> Result<Response, Error> {
        let mut request = RequestBuilder::new(self, method, path);
        for (key, value) in &path_params {
            request = request.param(key, value);
        }
        for (key, values) in &query_params {
            for value in values {
                request = request.param(key, value);
            }
        }
        for (key, value) in &headers {
            request.headers_mut().insert(key.clone(), value.clone());
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        let accept = match accept {
            Accept::Empty | Accept::Serializable => APPLICATION_JSON.clone(),
            Accept::Binary => APPLICATION_OCTET_STREAM.clone(),
        };
        request.headers_mut().insert(ACCEPT, accept);

        request.send().await
    }
}

impl AsyncClient for Client {
    type BinaryWriter = BodyWriter;
    type BinaryBody = ResponseBody;

    fn request<'a, T, U>(
        &'a self,
        method: Method,
        path: &'static str,
        path_params: PathParams,
        query_params: QueryParams,
        headers: HeaderMap<HeaderValue>,
        body: T,
        response_visitor: U,
    ) -> Pin<Box<dyn Future<Output = Result<U::Output, Error>> + Send + 'a>>
    where
        T: AsyncRequestBody<'a, Self::BinaryWriter> + Send + 'a,
        U: VisitResponse<Self::BinaryBody> + Send + 'a,
    {
        Box::pin(async move {
            let body = body.accept(RawBodyVisitor)?;
            let accept = response_visitor.accept();

            let response = self
                .conjure_inner(
                    method,
                    path,
                    path_params,
                    query_params,
                    headers,
                    body,
                    accept,
                )
                .await?;

            if response.status() == StatusCode::NO_CONTENT {
                return response_visitor.visit_empty();
            }

            if let Some(header) = response.headers().get(CONTENT_TYPE) {
                if header == *APPLICATION_JSON {
                    let mut buf = vec![];
                    let body = response.into_body();
                    pin_mut!(body);
                    body.read_to_end(&mut buf)
                        .await
                        .map_err(Error::internal_safe)?;
                    let mut deserializer = json::ClientDeserializer::from_slice(&buf);
                    let r = response_visitor.visit_serializable(&mut deserializer)?;
                    deserializer.end().map_err(Error::internal_safe)?;
                    return Ok(r);
                } else if header == *APPLICATION_OCTET_STREAM {
                    let body = response.into_body();
                    return response_visitor.visit_binary(body);
                }
            }

            Err(Error::internal_safe("invalid response Content-Type"))
        })
    }
}

enum RawBody<'a> {
    Json(Bytes),
    Binary(Pin<Box<dyn AsyncWriteBody<BodyWriter> + Sync + Send + 'a>>),
}

#[async_trait]
impl<'a> Body for RawBody<'a> {
    fn content_length(&self) -> Option<u64> {
        match self {
            RawBody::Json(buf) => Some(buf.len() as u64),
            RawBody::Binary(_) => None,
        }
    }

    fn content_type(&self) -> HeaderValue {
        match self {
            RawBody::Json(_) => APPLICATION_JSON.clone(),
            RawBody::Binary(_) => APPLICATION_OCTET_STREAM.clone(),
        }
    }

    fn full_body(&self) -> Option<Bytes> {
        match self {
            RawBody::Json(body) => Some(body.clone()),
            RawBody::Binary(_) => None,
        }
    }

    async fn write(mut self: Pin<&mut Self>, w: Pin<&mut BodyWriter>) -> Result<(), Error> {
        match &mut *self {
            RawBody::Json(_) => unreachable!(),
            RawBody::Binary(body) => body.as_mut().write_body(w).await,
        }
    }

    async fn reset(mut self: Pin<&mut Self>) -> bool {
        match &mut *self {
            RawBody::Json(_) => true,
            RawBody::Binary(body) => body.as_mut().reset().await,
        }
    }
}

struct RawBodyVisitor;

impl<'a> AsyncVisitRequestBody<'a, BodyWriter> for RawBodyVisitor {
    type Output = Result<Option<RawBody<'a>>, Error>;

    fn visit_empty(self) -> Self::Output {
        Ok(None)
    }

    fn visit_serializable<T>(self, body: T) -> Self::Output
    where
        T: Serialize + 'a,
    {
        let body = json::to_vec(&body).map_err(Error::internal)?;
        Ok(Some(RawBody::Json(Bytes::from(body))))
    }

    fn visit_binary<T>(self, body: T) -> Self::Output
    where
        T: AsyncWriteBody<BodyWriter> + Sync + Send + 'a,
    {
        Ok(Some(RawBody::Binary(Box::pin(body))))
    }
}
