use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("internal Tokio runtime must initialize")
    })
}

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    runtime().block_on(future)
}
