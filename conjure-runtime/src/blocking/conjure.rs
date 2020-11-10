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
use crate::blocking::{Body, BodyWriter, Client, Response, ResponseBody};
use crate::raw;
use crate::{APPLICATION_JSON, APPLICATION_OCTET_STREAM};
use bytes::Bytes;
use conjure_error::Error;
use conjure_http::client::{Accept, RequestBody, VisitRequestBody, VisitResponse, WriteBody};
use conjure_http::{PathParams, QueryParams};
use conjure_serde::json;
use hyper::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
use hyper::{HeaderMap, Method, StatusCode};
use serde::Serialize;
use std::error;
use std::io::Read;
use tower::Service;

impl<T, B> Client<T>
where
    T: Service<http::Request<raw::RawBody>, Response = http::Response<B>>
        + Clone
        + 'static
        + Sync
        + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    T::Future: Send,
    B: http_body::Body<Data = Bytes> + 'static + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    #[allow(clippy::too_many_arguments)]
    fn conjure_inner(
        &self,
        method: Method,
        path: &'static str,
        path_params: PathParams,
        query_params: QueryParams,
        headers: HeaderMap,
        body: Option<RawBody<'_>>,
        accept: Accept,
    ) -> Result<Response<B>, Error> {
        let mut request = self.request(method, path);
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
        match accept {
            Accept::Empty | Accept::Serializable => {
                request
                    .headers_mut()
                    .insert(ACCEPT, APPLICATION_JSON.clone());
            }
            Accept::Binary => {
                request
                    .headers_mut()
                    .insert(ACCEPT, APPLICATION_OCTET_STREAM.clone());
            }
        }

        request.send()
    }
}

impl<T, B> conjure_http::client::Client for Client<T>
where
    T: Service<http::Request<raw::RawBody>, Response = http::Response<B>>
        + Clone
        + 'static
        + Sync
        + Send,
    T::Error: Into<Box<dyn error::Error + Sync + Send>>,
    T::Future: Send,
    B: http_body::Body<Data = Bytes> + 'static + Send,
    B::Error: Into<Box<dyn error::Error + Sync + Send>>,
{
    type BinaryWriter = BodyWriter;
    type BinaryBody = ResponseBody<B>;

    fn request<'a, R, U>(
        &self,
        method: Method,
        path: &'static str,
        path_params: PathParams,
        query_params: QueryParams,
        headers: HeaderMap<HeaderValue>,
        body: R,
        response_visitor: U,
    ) -> Result<<U as VisitResponse<Self::BinaryBody>>::Output, Error>
    where
        R: RequestBody<'a, Self::BinaryWriter>,
        U: VisitResponse<Self::BinaryBody>,
    {
        let body = body.accept(RawBodyVisitor)?;
        let accept = response_visitor.accept();

        let response = self.conjure_inner(
            method,
            path,
            path_params,
            query_params,
            headers,
            body,
            accept,
        )?;

        if response.status() == StatusCode::NO_CONTENT {
            return response_visitor.visit_empty();
        }

        if let Some(header) = response.headers().get(CONTENT_TYPE) {
            if header == APPLICATION_JSON.as_ref() {
                let mut body = vec![];
                response
                    .into_body()
                    .read_to_end(&mut body)
                    .map_err(Error::internal_safe)?;
                let mut deserializer = json::ClientDeserializer::from_slice(&body);
                let r = response_visitor.visit_serializable(&mut deserializer)?;
                deserializer.end().map_err(Error::internal_safe)?;
                return Ok(r);
            } else if header == APPLICATION_OCTET_STREAM.as_ref() {
                let body = response.into_body();
                return response_visitor.visit_binary(body);
            }
        }

        Err(Error::internal_safe("invalid response Content-Type"))
    }
}

enum RawBody<'a> {
    Json(Bytes),
    Binary(Box<dyn WriteBody<BodyWriter> + 'a>),
}

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
            RawBody::Json(buf) => Some(buf.clone()),
            RawBody::Binary(_) => None,
        }
    }

    fn write(&mut self, w: &mut BodyWriter) -> Result<(), Error> {
        match self {
            RawBody::Json(_) => unreachable!(),
            RawBody::Binary(body) => body.write_body(w),
        }
    }

    fn reset(&mut self) -> bool {
        match self {
            RawBody::Json(_) => true,
            RawBody::Binary(body) => body.reset(),
        }
    }
}

struct RawBodyVisitor;

impl<'a> VisitRequestBody<'a, BodyWriter> for RawBodyVisitor {
    type Output = Result<Option<RawBody<'a>>, Error>;

    fn visit_empty(self) -> Result<Option<RawBody<'a>>, Error> {
        Ok(None)
    }

    fn visit_serializable<T>(self, body: T) -> Result<Option<RawBody<'a>>, Error>
    where
        T: Serialize + 'a,
    {
        let body = json::to_vec(&body).map_err(Error::internal)?;
        Ok(Some(RawBody::Json(Bytes::from(body))))
    }

    fn visit_binary<T>(self, body: T) -> Result<Option<RawBody<'a>>, Error>
    where
        T: WriteBody<BodyWriter> + 'a,
    {
        Ok(Some(RawBody::Binary(Box::new(body))))
    }
}
