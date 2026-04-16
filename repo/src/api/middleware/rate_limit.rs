/// Per-IP rate-limit middleware.
///
/// Applied globally before authentication so that even unauthenticated requests
/// (login, captcha, health) are subject to the IP limit.
///
/// Limit: 300 requests / minute per client IP.
///
/// The client IP is extracted from:
/// 1. `X-Forwarded-For` first value (when behind a trusted proxy), OR
/// 2. `connection_info().realip_remote_addr()`.
///
/// A request that exceeds the limit receives HTTP 429 with a `Retry-After` header
/// and the standard error envelope.
use std::future::{ready, Future, Ready};
use std::pin::Pin;

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    web, Error, HttpMessage,
};
use tracing::warn;

use crate::infrastructure::ratelimit;
use crate::shared::{app_state::AppState, errors::AppError};

pub struct IpRateLimitMiddleware;

impl<S, B> Transform<S, ServiceRequest> for IpRateLimitMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = IpRateLimitService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(IpRateLimitService {
            service: std::rc::Rc::new(service),
        }))
    }
}

pub struct IpRateLimitService<S> {
    service: std::rc::Rc<S>,
}

impl<S, B> Service<ServiceRequest> for IpRateLimitService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let ip = client_ip(&req);

        let state = match req.app_data::<web::Data<AppState>>() {
            Some(s) => s.clone(),
            None => {
                // If state is missing, let the request through (shouldn't happen).
                let fut = self.service.call(req);
                return Box::pin(async move { fut.await });
            }
        };

        if let Err(retry_after) = ratelimit::check_ip(&state.rate_limiters, &ip) {
            warn!(client_ip = %ip, retry_after_secs = retry_after, "IP rate limit exceeded");
            return Box::pin(ready(Err(actix_web::Error::from(AppError::RateLimited))));
        }

        let fut = self.service.call(req);
        Box::pin(async move { fut.await })
    }
}

/// Extract the real client IP, preferring `X-Forwarded-For` if set.
pub fn client_ip(req: &ServiceRequest) -> String {
    req.headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(str::trim)
        .map(str::to_string)
        .or_else(|| {
            req.connection_info()
                .realip_remote_addr()
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}
