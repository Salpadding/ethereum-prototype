use jsonrpc_core::futures::future::Either;
use jsonrpc_core::futures::Future;
use jsonrpc_core::*;
use std::sync::atomic::{self, AtomicUsize};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct Meta(usize);
impl Metadata for Meta {}

#[derive(Default)]
pub struct MyMiddleware(pub AtomicUsize);
impl Middleware<Meta> for MyMiddleware {
    type Future = FutureResponse;
    type CallFuture = middleware::NoopCallFuture;

    fn on_request<F, X>(&self, request: Request, meta: Meta, next: F) -> Either<Self::Future, X>
    where
        F: FnOnce(Request, Meta) -> X + Send,
        X: Future<Item = Option<Response>, Error = ()> + Send + 'static,
    {
        let start = Instant::now();
        let request_number = self.0.fetch_add(1, atomic::Ordering::SeqCst);
        // println!(
        //     "Processing request {}: {:?}, {:?}",
        //     request_number, request, meta
        // );

        Either::A(Box::new(next(request, meta).map(move |res| {
            // println!("Processing took: {:?}", start.elapsed());
            res
        })))
    }
}
