use crate::metrics::MetricsSnapshot;
use crate::storage::MetricsBuffer;
use nuts;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::warn;

#[derive(Clone, Debug)]
pub struct MetricsEvent(pub MetricsSnapshot);

pub fn register_storage_subscriber(
    buffer: Arc<MetricsBuffer>,
) -> nuts::ActivityId<Arc<MetricsBuffer>> {
    let activity = nuts::new_activity(buffer);
    activity.subscribe(move |buf: &mut Arc<MetricsBuffer>, evt: &MetricsEvent| {
        let snapshot = evt.0.clone();
        // Push synchronously; storage uses blocking RwLock.
        buf.push(snapshot);
    });
    activity
}

pub fn register_storage_and_stream_subscriber(
    buffer: Arc<MetricsBuffer>,
    stream_tx: broadcast::Sender<MetricsSnapshot>,
) -> nuts::ActivityId<Arc<MetricsBuffer>> {
    let activity = nuts::new_activity(buffer);
    activity.subscribe(move |buf: &mut Arc<MetricsBuffer>, evt: &MetricsEvent| {
        let snapshot = evt.0.clone();
        buf.push(snapshot.clone());

        if let Err(e) = stream_tx.send(snapshot) {
            // This happens if there are no receivers or they lag behind; keep storage as source of truth.
            warn!("Failed to broadcast snapshot to stream: {}", e);
        }
    });
    activity
}

pub fn publish_snapshot(snapshot: MetricsSnapshot) {
    nuts::publish(MetricsEvent(snapshot));
}
