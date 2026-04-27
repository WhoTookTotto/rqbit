/// Async-safe network namespace helper.
///
/// `setns()` (used by `netns_rs::NetNs::run()`) only affects the *calling OS thread*.
/// Calling it directly from a Tokio worker thread would briefly move all tasks scheduled
/// on that thread into the wrong namespace. Instead, we use `tokio::task::spawn_blocking`
/// to run socket creation on a dedicated blocking-pool thread, isolating the `setns` effect.
///
/// After creation the socket fd permanently belongs to the namespace it was created in,
/// so the resulting socket can be used freely from any Tokio worker thread.
use std::sync::Arc;

pub(crate) use netns_rs::NetNs;

/// Run `f` inside the given network namespace.
///
/// If `ns` is `None`, `f` is called directly in the current namespace.
/// If `ns` is `Some`, `f` is executed on a `spawn_blocking` thread that enters the
/// namespace, runs `f`, then restores the original namespace before returning.
pub(crate) async fn run_in_netns<F, R>(ns: Option<&Arc<NetNs>>, f: F) -> anyhow::Result<R>
where
    F: FnOnce() -> anyhow::Result<R> + Send + 'static,
    R: Send + 'static,
{
    match ns {
        None => f(),
        Some(ns) => {
            let ns = ns.clone();
            tokio::task::spawn_blocking(move || {
                ns.run(|_| f())
                    .map_err(|e| anyhow::anyhow!("netns enter failed: {e}"))?
            })
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?
        }
    }
}
