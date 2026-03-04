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
    F: Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::scope(|scope| {
            scope
                .spawn(|| runtime().block_on(future))
                .join()
                .expect("runtime helper thread must not panic")
        });
    }

    runtime().block_on(future)
}

#[cfg(test)]
mod tests {
    #[tokio::test(flavor = "current_thread")]
    async fn block_on_runs_inside_current_thread_runtime() {
        let text = String::from("hello");

        let len = super::block_on(async { text.len() });

        assert_eq!(len, 5);
    }
}
