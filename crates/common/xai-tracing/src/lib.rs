mod dispatch;
mod timer;

pub mod fastrace;
pub mod http_client;
pub mod tokio;

#[cfg(feature = "otlp")]
mod grpc_client;

#[cfg(all(test, feature = "otlp"))]
mod testing;

pub use dispatch::*;
pub use fastrace::*;
pub use http_client::{
    TracedHttpClient, attach_trace_to_http_request, traced_client, traced_client_from_builder,
    traced_client_new,
};
pub use timer::*;

#[cfg(feature = "otlp")]
pub use grpc_client::*;
